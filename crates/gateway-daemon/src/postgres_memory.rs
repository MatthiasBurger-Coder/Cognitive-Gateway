//! Durable governed-memory projection for reference-only experience payloads.
use gateway_application::memory::{CurationDecision, MemoryError, MemoryStore};
use gateway_domain::{
    ContextScopeId, ReferenceId,
    memory::{CurationState, ExperienceRecord, MEMORY_SCHEMA_VERSION, MemoryEntry, MemoryPayload},
};
use postgres::{Client, NoTls, Row};
use std::sync::Mutex;

pub struct PostgresMemoryStore {
    client: Mutex<Client>,
}

impl PostgresMemoryStore {
    pub fn connect(connection_string: &str) -> Result<Self, postgres::Error> {
        Self::from_client(Client::connect(connection_string, NoTls)?)
    }

    pub fn connect_config(config: &postgres::Config) -> Result<Self, postgres::Error> {
        Self::from_client(config.connect(NoTls)?)
    }

    fn from_client(mut client: Client) -> Result<Self, postgres::Error> {
        client.batch_execute(include_str!("../migrations/002_governed_memory.sql"))?;
        Ok(Self {
            client: Mutex::new(client),
        })
    }
}

fn state(value: CurationState) -> &'static str {
    match value {
        CurationState::Pending => "PENDING",
        CurationState::Validated => "VALIDATED",
        CurationState::Rejected => "REJECTED",
        CurationState::Invalidated => "INVALIDATED",
        CurationState::Superseded => "SUPERSEDED",
        CurationState::Forgotten => "FORGOTTEN",
    }
}

fn parse_state(value: &str) -> Result<CurationState, MemoryError> {
    match value {
        "PENDING" => Ok(CurationState::Pending),
        "VALIDATED" => Ok(CurationState::Validated),
        "REJECTED" => Ok(CurationState::Rejected),
        "INVALIDATED" => Ok(CurationState::Invalidated),
        "SUPERSEDED" => Ok(CurationState::Superseded),
        "FORGOTTEN" => Ok(CurationState::Forgotten),
        _ => Err(MemoryError::Storage),
    }
}

fn number(value: u64) -> Result<i64, MemoryError> {
    i64::try_from(value).map_err(|_| MemoryError::InvalidRecord)
}

fn decode(row: Row) -> Result<MemoryEntry, MemoryError> {
    let state = parse_state(row.get::<_, &str>(4))?;
    let raw: String = row.get(6);
    let mut value: serde_json::Value =
        serde_json::from_str(&raw).map_err(|_| MemoryError::Storage)?;
    if state == CurationState::Forgotten {
        if !value.get("payload").is_some_and(serde_json::Value::is_null) {
            return Err(MemoryError::Storage);
        }
        value["payload"] = serde_json::json!({"kind":"REFERENCE","value":"forgotten-placeholder"});
    }
    let mut record =
        ExperienceRecord::from_json(&value.to_string()).map_err(|_| MemoryError::Storage)?;
    if state == CurationState::Forgotten {
        record.payload = None;
    }
    let scope = ContextScopeId::new(row.get::<_, String>(0)).map_err(|_| MemoryError::Storage)?;
    let id = ReferenceId::new(row.get::<_, String>(1)).map_err(|_| MemoryError::Storage)?;
    let source_snapshot =
        ReferenceId::new(row.get::<_, String>(5)).map_err(|_| MemoryError::Storage)?;
    let revision: i64 = row.get(2);
    let eligibility_version: i64 = row.get(3);
    let payload_forgotten: bool = row.get(8);
    if record.scope != scope
        || record.id != id
        || record.source_snapshot != source_snapshot
        || revision < 1
        || eligibility_version < 1
        || payload_forgotten != (state == CurationState::Forgotten)
    {
        return Err(MemoryError::Storage);
    }
    let superseded_by = row
        .get::<_, Option<String>>(7)
        .map(ReferenceId::new)
        .transpose()
        .map_err(|_| MemoryError::Storage)?;
    Ok(MemoryEntry {
        schema_version: MEMORY_SCHEMA_VERSION,
        revision: u64::try_from(revision).map_err(|_| MemoryError::Storage)?,
        record,
        state,
        superseded_by,
        eligibility_version: u64::try_from(eligibility_version)
            .map_err(|_| MemoryError::Storage)?,
        payload_forgotten,
    })
}

fn encode(entry: &MemoryEntry) -> Result<String, MemoryError> {
    if entry.state == CurationState::Forgotten {
        if entry.record.payload.is_some() || !entry.payload_forgotten {
            return Err(MemoryError::InvalidRecord);
        }
        let mut temporary = entry.record.clone();
        temporary.payload = Some(MemoryPayload::Reference(
            ReferenceId::new("forgotten-placeholder").expect("static reference is valid"),
        ));
        let mut value: serde_json::Value = serde_json::from_str(
            &temporary
                .to_json()
                .map_err(|_| MemoryError::InvalidRecord)?,
        )
        .map_err(|_| MemoryError::InvalidRecord)?;
        value["payload"] = serde_json::Value::Null;
        Ok(value.to_string())
    } else {
        if !matches!(entry.record.payload, Some(MemoryPayload::Reference(_)))
            || entry.payload_forgotten
        {
            return Err(MemoryError::InvalidRecord);
        }
        entry
            .record
            .to_json()
            .map_err(|_| MemoryError::InvalidRecord)
    }
}

const SELECT: &str = "SELECT scope, id, revision, eligibility_version, state, source_snapshot, record_json, superseded_by, payload_forgotten FROM cg_memory_entries";

impl MemoryStore for PostgresMemoryStore {
    fn get(
        &self,
        scope: &ContextScopeId,
        id: &ReferenceId,
    ) -> Result<Option<MemoryEntry>, MemoryError> {
        let mut client = self.client.lock().map_err(|_| MemoryError::Storage)?;
        let query = format!("{SELECT} WHERE scope = $1 AND id = $2");
        client
            .query_opt(&query, &[&scope.as_str(), &id.as_str()])
            .map_err(|_| MemoryError::Storage)?
            .map(decode)
            .transpose()
    }

    fn list(&self, scope: &ContextScopeId) -> Result<Vec<MemoryEntry>, MemoryError> {
        let mut client = self.client.lock().map_err(|_| MemoryError::Storage)?;
        let query = format!("{SELECT} WHERE scope = $1 ORDER BY id");
        client
            .query(&query, &[&scope.as_str()])
            .map_err(|_| MemoryError::Storage)?
            .into_iter()
            .map(decode)
            .collect()
    }

    fn commit(
        &mut self,
        expected: Option<u64>,
        entry: MemoryEntry,
        decision: CurationDecision,
    ) -> Result<(), MemoryError> {
        if entry.schema_version != MEMORY_SCHEMA_VERSION
            || entry.record.scope != decision.scope
            || entry.record.id != decision.id
            || entry.revision != decision.output_revision
            || decision.input_revision != expected
            || decision.output_id != entry.record.id
        {
            return Err(MemoryError::InvalidRecord);
        }
        let record_json = encode(&entry)?;
        let mut client = self.client.lock().map_err(|_| MemoryError::Storage)?;
        let mut tx = client.transaction().map_err(|_| MemoryError::Storage)?;
        tx.query_one(
            "SELECT pg_advisory_xact_lock(hashtext($1)::bigint)",
            &[&entry.record.scope.as_str()],
        )
        .map_err(|_| MemoryError::Storage)?;
        let previous = tx
            .query_opt(
                "SELECT revision FROM cg_memory_entries WHERE scope = $1 AND id = $2 FOR UPDATE",
                &[&entry.record.scope.as_str(), &entry.record.id.as_str()],
            )
            .map_err(|_| MemoryError::Storage)?;
        let previous = previous.map(|row| row.get::<_, i64>(0));
        if previous != expected.map(number).transpose()? {
            return Err(MemoryError::RevisionConflict);
        }
        let revision = number(entry.revision)?;
        let eligibility_version = number(entry.eligibility_version)?;
        let superseded_by = entry.superseded_by.as_ref().map(ReferenceId::as_str);
        let result = tx.execute(
            "INSERT INTO cg_memory_entries (scope, id, revision, eligibility_version, state, source_snapshot, record_json, superseded_by, payload_forgotten) VALUES ($1,$2,$3,$4,$5,$6,$7,$8,$9) ON CONFLICT (scope,id) DO UPDATE SET revision=EXCLUDED.revision, eligibility_version=EXCLUDED.eligibility_version, state=EXCLUDED.state, source_snapshot=EXCLUDED.source_snapshot, record_json=EXCLUDED.record_json, superseded_by=EXCLUDED.superseded_by, payload_forgotten=EXCLUDED.payload_forgotten",
            &[&entry.record.scope.as_str(), &entry.record.id.as_str(), &revision, &eligibility_version,
                &state(entry.state), &entry.record.source_snapshot.as_str(), &record_json,
                &superseded_by, &entry.payload_forgotten],
        );
        if let Err(error) = result {
            return Err(
                if error.code() == Some(&postgres::error::SqlState::UNIQUE_VIOLATION) {
                    MemoryError::Conflict
                } else {
                    MemoryError::Storage
                },
            );
        }
        let decision_json = serde_json::to_string(&decision).map_err(|_| MemoryError::Storage)?;
        tx.execute("INSERT INTO cg_memory_decisions (scope,id,output_revision,decision_json) VALUES ($1,$2,$3,$4)",
            &[&decision.scope.as_str(), &decision.id.as_str(), &revision, &decision_json])
            .map_err(|_| MemoryError::Storage)?;
        tx.commit().map_err(|_| MemoryError::Storage)
    }

    fn decisions(
        &self,
        scope: &ContextScopeId,
        id: &ReferenceId,
    ) -> Result<Vec<CurationDecision>, MemoryError> {
        let mut client = self.client.lock().map_err(|_| MemoryError::Storage)?;
        client.query("SELECT decision_json FROM cg_memory_decisions WHERE scope = $1 AND id = $2 ORDER BY output_revision",
            &[&scope.as_str(), &id.as_str()]).map_err(|_| MemoryError::Storage)?
            .into_iter().map(|row| serde_json::from_str(&row.get::<_, String>(0)).map_err(|_| MemoryError::Storage)).collect()
    }
}
