//! Transactional, scoped cognitive journals. Database credentials belong only to
//! the trusted coordinator; workers/models never receive this adapter.
use gateway_domain::ContextScopeId;
use postgres::{Client, NoTls};
use serde::{Serialize, de::DeserializeOwned};
use sha2::{Digest, Sha256};
use std::{sync::Mutex, time::Duration};

pub struct CognitiveStore {
    client: Mutex<Client>,
    scope: ContextScopeId,
}
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum StoreError {
    Storage,
    InvalidJournal,
    Limit,
    CommitUnknown,
}
impl CognitiveStore {
    pub fn connect(connection: &str, scope: ContextScopeId) -> Result<Self, StoreError> {
        let mut configuration = connection
            .parse::<postgres::Config>()
            .map_err(|_| StoreError::Storage)?;
        configuration
            .connect_timeout(Duration::from_secs(3))
            .tcp_user_timeout(Duration::from_secs(5));
        let mut client = configuration
            .connect(NoTls)
            .map_err(|_| StoreError::Storage)?;
        client
            .batch_execute("SET statement_timeout = '5s'; SET lock_timeout = '5s'")
            .map_err(|_| StoreError::Storage)?;
        client
            .batch_execute(
                "CREATE TABLE IF NOT EXISTS cg_cognitive_journals (
            scope TEXT NOT NULL, kind TEXT NOT NULL,
            revision BIGINT NOT NULL CHECK (revision >= 0),
            payload TEXT NOT NULL CHECK (octet_length(payload) <= 33554432),
            digest TEXT NOT NULL, PRIMARY KEY(scope, kind))",
            )
            .map_err(|_| StoreError::Storage)?;
        Ok(Self {
            client: Mutex::new(client),
            scope,
        })
    }
    pub fn scope(&self) -> &ContextScopeId {
        &self.scope
    }
    /// Row lock serializes coordinators. Projection and routing change commit
    /// together. No cache is exposed before commit; a failed operation rolls back.
    pub fn transact<T, R, E>(
        &self,
        kind: &str,
        initial: &T,
        operation: impl FnOnce(&mut T) -> Result<R, E>,
    ) -> Result<R, E>
    where
        T: Serialize + DeserializeOwned,
        E: From<StoreError>,
    {
        let raw = serde_json::to_string(initial).map_err(|_| StoreError::InvalidJournal)?;
        let hash = format!("{:x}", Sha256::digest(raw.as_bytes()));
        let mut client = self.client.lock().map_err(|_| StoreError::Storage)?;
        let mut tx = client.transaction().map_err(|_| StoreError::Storage)?;
        tx.execute(
            "INSERT INTO cg_cognitive_journals VALUES ($1,$2,0,$3,$4) ON CONFLICT DO NOTHING",
            &[&self.scope.as_str(), &kind, &raw, &hash],
        )
        .map_err(|_| StoreError::Storage)?;
        let row = tx.query_one("SELECT payload,digest,revision FROM cg_cognitive_journals WHERE scope=$1 AND kind=$2 FOR UPDATE",
            &[&self.scope.as_str(), &kind]).map_err(|_| StoreError::Storage)?;
        let payload: String = row.get(0);
        let expected: String = row.get(1);
        if expected != format!("{:x}", Sha256::digest(payload.as_bytes())) {
            return Err(StoreError::InvalidJournal.into());
        }
        let revision: i64 = row.get(2);
        let mut state: T =
            serde_json::from_str(&payload).map_err(|_| StoreError::InvalidJournal)?;
        let result = operation(&mut state)?;
        let next = serde_json::to_string(&state).map_err(|_| StoreError::InvalidJournal)?;
        if next.len() > 33_554_432 {
            return Err(StoreError::Limit.into());
        }
        if next != payload {
            let revision = revision.checked_add(1).ok_or(StoreError::Limit)?;
            let hash = format!("{:x}", Sha256::digest(next.as_bytes()));
            tx.execute("UPDATE cg_cognitive_journals SET payload=$3,digest=$4,revision=$5 WHERE scope=$1 AND kind=$2",
                &[&self.scope.as_str(), &kind, &next, &hash, &revision]).map_err(|_| StoreError::Storage)?;
        }
        tx.commit().map_err(|_| StoreError::CommitUnknown)?;
        Ok(result)
    }
}
