//! PostgreSQL persistence for references admitted by governed memory.
use gateway_application::{
    closed_loop::ClosedLoop,
    experience_patterns::{
        ExperienceIngestionPort, OutcomeClass, PatternError, PatternLimits, VerifiedExecution,
        inspect_patterns,
    },
    memory::{MemoryApplication, MemoryStore},
};
use gateway_domain::{
    ContextScopeId, ReferenceId, UnixTimestamp, learning::FingerprintSignal,
    memory::MemoryEligibilityReference,
};
use postgres::{Client, NoTls};
use std::sync::Mutex;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RetentionPolicy {
    pub max_rows_per_scope: i64,
    pub max_age_seconds: i64,
}

impl Default for RetentionPolicy {
    fn default() -> Self {
        Self {
            max_rows_per_scope: 10_000,
            max_age_seconds: 90 * 24 * 60 * 60,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct AdmissionReceipt {
    pub inserted: bool,
    pub pruned_rows: u64,
}

#[derive(Debug)]
pub enum PostgresExperienceError {
    Database(postgres::Error),
    Pattern(PatternError),
    Serialization(serde_json::Error),
    InvalidRetention,
    Ineligible,
    Unverified,
    Conflict,
    Capacity,
    Storage,
}

impl From<postgres::Error> for PostgresExperienceError {
    fn from(error: postgres::Error) -> Self {
        Self::Database(error)
    }
}
impl From<serde_json::Error> for PostgresExperienceError {
    fn from(error: serde_json::Error) -> Self {
        Self::Serialization(error)
    }
}
impl From<PatternError> for PostgresExperienceError {
    fn from(error: PatternError) -> Self {
        Self::Pattern(error)
    }
}

/// One connection is shared behind a mutex because the application read port
/// accepts `&self`. The database transaction serializes admissions per scope.
pub struct PostgresExperienceStore {
    client: Mutex<Client>,
    retention: RetentionPolicy,
}

impl PostgresExperienceStore {
    /// This local Compose adapter uses the synchronous client without TLS.
    /// A remote deployment should supply a TLS-enabled adapter.
    pub fn connect(
        connection_string: &str,
        retention: RetentionPolicy,
    ) -> Result<Self, PostgresExperienceError> {
        Self::from_client(Client::connect(connection_string, NoTls)?, retention)
    }

    pub fn connect_config(
        config: &postgres::Config,
        retention: RetentionPolicy,
    ) -> Result<Self, PostgresExperienceError> {
        Self::from_client(config.connect(NoTls)?, retention)
    }

    fn from_client(
        mut client: Client,
        retention: RetentionPolicy,
    ) -> Result<Self, PostgresExperienceError> {
        if retention.max_rows_per_scope < 1 || retention.max_age_seconds < 1 {
            return Err(PostgresExperienceError::InvalidRetention);
        }
        client.batch_execute(include_str!("../migrations/001_verified_executions.sql"))?;
        Ok(Self {
            client: Mutex::new(client),
            retention,
        })
    }

    /// Persists a verdict issued by the Closed Loop only when its governed
    /// memory record pins the same source, validation, outcome and label evidence.
    pub fn record_closed_loop<S: MemoryStore>(
        &self,
        memory: &MemoryApplication<S>,
        run: &ClosedLoop,
        memory_id: &ReferenceId,
        runtime: ReferenceId,
        evaluation: ReferenceId,
        at: UnixTimestamp,
    ) -> Result<AdmissionReceipt, PostgresExperienceError> {
        let receipt = run
            .verified_outcome()
            .ok_or(PostgresExperienceError::Unverified)?;
        let (entry, _) = memory
            .inspect(receipt.scope(), memory_id, at)
            .map_err(|error| PostgresExperienceError::Pattern(PatternError::Memory(error)))?;
        let record = &entry.record;
        let label = record
            .label_basis
            .clone()
            .ok_or(PostgresExperienceError::Ineligible)?;
        let outcome = match receipt.outcome() {
            OutcomeClass::Success => "SUCCESS",
            OutcomeClass::Failure => "FAILURE",
        };
        if record.source_snapshot != *receipt.source_snapshot()
            || record.source_digest != *receipt.source_digest()
            || record.validation.as_ref() != Some(receipt.validation())
            || record.outcome.as_ref().map(|value| value.as_str()) != Some(outcome)
            || !receipt
                .evidence()
                .iter()
                .any(|evidence| evidence.as_str() == label.as_str())
        {
            return Err(PostgresExperienceError::Unverified);
        }
        let execution = VerifiedExecution {
            memory_id: memory_id.clone(),
            runtime,
            trace: receipt.source_snapshot().clone(),
            evaluation,
            validation: receipt.validation().clone(),
            label_basis: label,
            evidence: receipt.evidence().to_vec(),
            signals: receipt
                .facts()
                .iter()
                .cloned()
                .map(FingerprintSignal::Fact)
                .collect(),
            semantic_hint: None,
        };
        self.record_verified(memory, receipt.scope(), at, execution)
    }

    /// Accepts a verified outcome only while its governed source is currently
    /// learning eligible. A retry with identical canonical content is idempotent.
    pub fn record_verified<S: MemoryStore>(
        &self,
        memory: &MemoryApplication<S>,
        scope: &ContextScopeId,
        at: UnixTimestamp,
        mut execution: VerifiedExecution,
    ) -> Result<AdmissionReceipt, PostgresExperienceError> {
        struct One(VerifiedExecution);
        impl ExperienceIngestionPort for One {
            fn list_verified(
                &self,
                _: &ContextScopeId,
                _: PatternLimits,
            ) -> Result<Vec<VerifiedExecution>, PatternError> {
                Ok(vec![self.0.clone()])
            }
        }
        let report = inspect_patterns(
            memory,
            &One(execution.clone()),
            scope,
            at,
            PatternLimits {
                max_inputs: 1,
                ..PatternLimits::default()
            },
        )?;
        if report.metrics.eligible_count != 1 {
            return Err(PostgresExperienceError::Ineligible);
        }
        let finding = &report.findings[0];
        let normalized = finding
            .successes
            .first()
            .or_else(|| finding.failures.first())
            .ok_or(PostgresExperienceError::Ineligible)?;
        execution.signals = normalized.fingerprint.signals().to_vec();
        execution.evidence = normalized.evidence.clone();
        execution.semantic_hint = normalized.semantic_hint.clone();
        let eligibility = normalized.basis.memory();
        let payload = serde_json::to_string(&execution)?;
        let eligibility_json = serde_json::to_string(eligibility)?;
        let cutoff = at.seconds().saturating_sub(self.retention.max_age_seconds);
        let mut client = self
            .client
            .lock()
            .map_err(|_| PostgresExperienceError::Storage)?;
        let mut transaction = client.transaction()?;
        transaction.query_one(
            "SELECT pg_advisory_xact_lock(hashtext($1)::bigint)",
            &[&scope.as_str()],
        )?;
        let pruned_rows = transaction.execute(
            "DELETE FROM cg_verified_executions WHERE scope = $1 AND recorded_at < $2",
            &[&scope.as_str(), &cutoff],
        )?;
        if let Some(row) = transaction.query_opt(
            "SELECT execution_json, eligibility_json FROM cg_verified_executions WHERE scope = $1 AND memory_id = $2 FOR UPDATE",
            &[&scope.as_str(), &execution.memory_id.as_str()],
        )? {
            let same = row.get::<_, String>(0) == payload && row.get::<_, String>(1) == eligibility_json;
            if !same { return Err(PostgresExperienceError::Conflict); }
            transaction.commit()?;
            return Ok(AdmissionReceipt { inserted: false, pruned_rows });
        }
        let count: i64 = transaction
            .query_one(
                "SELECT count(*) FROM cg_verified_executions WHERE scope = $1",
                &[&scope.as_str()],
            )?
            .get(0);
        if count >= self.retention.max_rows_per_scope {
            return Err(PostgresExperienceError::Capacity);
        }
        let inserted = transaction.execute(
            "INSERT INTO cg_verified_executions (scope, memory_id, trace, recorded_at, execution_json, eligibility_json) VALUES ($1, $2, $3, $4, $5, $6)",
            &[&scope.as_str(), &execution.memory_id.as_str(), &execution.trace.as_str(), &at.seconds(), &payload, &eligibility_json],
        );
        match inserted {
            Ok(1) => {
                transaction.commit()?;
                Ok(AdmissionReceipt {
                    inserted: true,
                    pruned_rows,
                })
            }
            Ok(_) => Err(PostgresExperienceError::Storage),
            Err(error) if error.code() == Some(&postgres::error::SqlState::UNIQUE_VIOLATION) => {
                Err(PostgresExperienceError::Conflict)
            }
            Err(error) => Err(PostgresExperienceError::Database(error)),
        }
    }

    pub fn count(&self, scope: &ContextScopeId) -> Result<i64, PostgresExperienceError> {
        let mut client = self
            .client
            .lock()
            .map_err(|_| PostgresExperienceError::Storage)?;
        Ok(client
            .query_one(
                "SELECT count(*) FROM cg_verified_executions WHERE scope = $1",
                &[&scope.as_str()],
            )?
            .get(0))
    }
}

impl ExperienceIngestionPort for PostgresExperienceStore {
    fn list_verified(
        &self,
        scope: &ContextScopeId,
        limits: PatternLimits,
    ) -> Result<Vec<VerifiedExecution>, PatternError> {
        let maximum = i64::try_from(
            limits
                .max_inputs
                .checked_add(1)
                .ok_or(PatternError::InvalidLimits)?,
        )
        .map_err(|_| PatternError::InvalidLimits)?;
        let mut client = self.client.lock().map_err(|_| PatternError::Storage)?;
        let rows = client.query(
            "SELECT memory_id, trace, execution_json, eligibility_json FROM cg_verified_executions WHERE scope = $1 ORDER BY memory_id LIMIT $2",
            &[&scope.as_str(), &maximum],
        ).map_err(|_| PatternError::Storage)?;
        if rows.len() > limits.max_inputs {
            return Err(PatternError::TooManyInputs);
        }
        let mut output = Vec::with_capacity(rows.len());
        for row in rows {
            let execution: VerifiedExecution = serde_json::from_str(&row.get::<_, String>(2))
                .map_err(|_| PatternError::Storage)?;
            let eligibility: MemoryEligibilityReference =
                serde_json::from_str(&row.get::<_, String>(3))
                    .map_err(|_| PatternError::Storage)?;
            let memory_id =
                ReferenceId::new(row.get::<_, String>(0)).map_err(|_| PatternError::Storage)?;
            let trace =
                ReferenceId::new(row.get::<_, String>(1)).map_err(|_| PatternError::Storage)?;
            if execution.memory_id != memory_id
                || execution.trace != trace
                || eligibility.scope != *scope
                || eligibility.id != memory_id
                || eligibility.source_snapshot != trace
            {
                return Err(PatternError::Storage);
            }
            output.push(execution);
        }
        Ok(output)
    }
}
