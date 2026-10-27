//! Qualified local scalar acquisition bounds. No provider billing or embedding
//! service is included. Model context/window accounting remains independent.
use super::{
    ConnectorAdmission, Error, HostConfig, McpConnectorHost, Observer, ProtocolResult,
    mapping::{
        CanonicalConnectorMapping, ConnectorResultNormalizer, DiscoveryMapper, MappedInventory,
        MappedInventoryCache, MappedToolSchema, MappingDocument, MappingRecord,
    },
    normalization::CanonicalResultNormalizer,
    session_budget::{ReadonlyAllocationView, SessionConnectorQuote, SessionConnectorQuotePort},
    transport::TransportFactory,
};
use gateway_application::{
    connectors::*,
    sessions::{SessionError, SessionResourceAllocation, SessionResourceAmounts},
};
use gateway_domain::*;
use serde_json::json;
use std::{
    collections::BTreeMap,
    sync::{Arc, Weak},
};

/// Explicit operator tariff for a local, fixed-price, no-embedding operation.
/// Stdio transport alone never establishes this pricing contract. Variable or
/// externally billed services require another qualified accounting adapter.
pub(crate) struct FixedLocalTariff {
    unit: ReferenceId,
    discovery: u64,
    operation: u64,
}
impl FixedLocalTariff {
    pub(crate) fn operator_fixed_no_embedding(
        unit: ReferenceId,
        discovery: u64,
        operation: u64,
    ) -> Self {
        Self {
            unit,
            discovery,
            operation,
        }
    }
}

/// Source-owned response membership validation. This pure callback may reject
/// immutable provider data, but must perform no provider/model/embedding I/O.
pub(crate) trait ScalarResponseAdmission: Send + Sync {
    fn validate(
        &self,
        request: &ConnectorRequest,
        source: Option<&SourceId>,
        result: &ProtocolResult,
    ) -> Result<(), ConnectorFailure>;
}

/// Host and quote proof are assembled together: callers cannot substitute an
/// arbitrary normalizer, mapper or transport limit underneath these bounds.
pub(crate) struct QualifiedScalarHost {
    host: Arc<McpConnectorHost>,
    quotes: Arc<dyn SessionConnectorQuotePort>,
    metadata: QualifiedScalarMetadata,
}
/// Non-cloneable proof of the concrete immutable quote implementation. Only
/// consuming a qualified scalar host can produce it; no caller flag/blanket port
/// implementation can assert that arbitrary quote callbacks are pure.
pub(super) struct PureQuoteProof {
    _sealed: (),
}

impl QualifiedScalarHost {
    pub(crate) fn metadata(&self) -> QualifiedScalarMetadata {
        self.metadata.clone()
    }
    pub(super) fn into_parts(
        self,
    ) -> (
        Arc<McpConnectorHost>,
        Arc<dyn SessionConnectorQuotePort>,
        PureQuoteProof,
    ) {
        (self.host, self.quotes, PureQuoteProof { _sealed: () })
    }
}
/// Metadata access cannot discover, connect or invoke a provider. The weak host
/// reference prevents a retained witness from extending the transport lifetime.
#[derive(Clone)]
pub(crate) struct QualifiedScalarMetadata {
    host: Weak<McpConnectorHost>,
    retained: Arc<MappedInventoryCache>,
}
impl QualifiedScalarMetadata {
    pub(crate) fn snapshot_for(
        &self,
        summary: &ConnectorInventory,
    ) -> Result<MappedInventory, Error> {
        let _host = self.host.upgrade().ok_or(Error::Unavailable)?;
        self.retained.snapshot_for(summary)
    }
    pub(crate) fn tool_schema(
        &self,
        target: &ConnectorTarget,
        capability: &CapabilityId,
        revision: &ContentDigest,
    ) -> Result<MappedToolSchema, Error> {
        self.host
            .upgrade()
            .ok_or(Error::Unavailable)?
            .tool_schema(target, capability, revision)
    }
}
pub(crate) struct QualifiedScalarConfig {
    pub(crate) allocation: ReadonlyAllocationView,
    pub(crate) host: HostConfig,
    pub(crate) document: MappingDocument,
    pub(crate) normalizers: BTreeMap<CapabilityId, CanonicalResultNormalizer>,
    pub(crate) tariff: FixedLocalTariff,
}
pub(crate) fn qualified_scalar_host(
    config: QualifiedScalarConfig,
    factory: Box<dyn TransportFactory>,
    admission: Arc<dyn ConnectorAdmission>,
    response_admission: Arc<dyn ScalarResponseAdmission>,
    observer: Arc<dyn Observer>,
) -> Result<QualifiedScalarHost, Error> {
    let QualifiedScalarConfig {
        allocation,
        host: config,
        document,
        normalizers,
        tariff,
    } = config;
    // SAME immutable instances are retained by the routing normalizer and quotes.
    // All configured mappings must have a qualified scalar result path.
    if normalizers.len() != document.records().len()
        || document.records().iter().any(|r| {
            normalizers.get(&r.capability).is_none_or(|n| {
                n.resource_bounds().is_err()
                    || n.resource_bounds()
                        .is_ok_and(|b| r.source.as_ref() != Some(b.source()))
            })
        })
    {
        return Err(Error::Configuration);
    }
    let mapper = DiscoveryMapper::new(
        config.target.clone(),
        config.manifest.clone(),
        document.clone(),
    )
    .map_err(|_| Error::Configuration)?;
    let shared = Arc::new(ScalarQuotes {
        allocation,
        target: config.target.clone(),
        document,
        normalizers,
        tariff,
        response_admission,
    });
    let retained = Arc::new(MappedInventoryCache::default());
    let mapping = CanonicalConnectorMapping::new_retaining(
        mapper,
        Box::new(Routed(shared.clone())),
        retained.clone(),
    );
    let host = Arc::new(McpConnectorHost::new(
        config,
        factory,
        admission,
        Box::new(mapping),
        observer,
    )?);
    let metadata = QualifiedScalarMetadata {
        host: Arc::downgrade(&host),
        retained,
    };
    Ok(QualifiedScalarHost {
        metadata,
        host,
        quotes: shared,
    })
}
struct Routed(Arc<ScalarQuotes>);
impl ConnectorResultNormalizer for Routed {
    fn normalize(
        &self,
        d: &ConnectorDispatch,
        source: Option<&SourceId>,
        mapping: &MappingRecord,
        result: ProtocolResult,
    ) -> Result<ConnectorObservation, Error> {
        self.0
            .response_admission
            .validate(&d.request, source, &result)
            .map_err(|f| Error::SessionDenied(f.code))?;
        self.0
            .normalizers
            .get(&d.request.capability)
            .ok_or(Error::Unsupported)?
            .canonicalize(d, source, mapping, result)
            .map(|r| r.into_observation())
    }
}
struct ScalarQuotes {
    allocation: ReadonlyAllocationView,
    target: ConnectorTarget,
    document: MappingDocument,
    normalizers: BTreeMap<CapabilityId, CanonicalResultNormalizer>,
    tariff: FixedLocalTariff,
    response_admission: Arc<dyn ScalarResponseAdmission>,
}
fn value_bytes(
    mut values: impl Iterator<Item = impl serde::Serialize>,
) -> Result<u64, SessionError> {
    values.try_fold(0u64, |sum, v| {
        sum.checked_add(
            serde_json::to_vec(&v)
                .map_err(|_| SessionError::InvalidInput)?
                .len() as u64,
        )
        .ok_or(SessionError::LimitExceeded)
    })
}
fn argument_bytes(request: &ConnectorRequest) -> Result<u64, SessionError> {
    // Native typed values, including every argument key/selector/query. This
    // accounts repeated input on every logical call; there is no embedding work.
    let args: BTreeMap<_, _> = request
        .arguments
        .iter()
        .map(|(k, v)| (k.as_str(), v))
        .collect();
    Ok(serde_json::to_vec(&args)
        .map_err(|_| SessionError::InvalidInput)?
        .len() as u64)
}
fn token_bound(bytes: u64) -> Result<u64, SessionError> {
    TokenEstimate {
        estimator: TokenEstimatorId::new("cg.connector.scalar-utf8").map_err(|_| SessionError::InvalidInput)?,
        version: TokenEstimatorVersion::new("1").map_err(|_| SessionError::InvalidInput)?,
        count: TokenCount::Estimated {
            tokens: bytes,
            upper_bound: Some(bytes),
            semantics: NonEmptyText::new("Conservative UTF8-byte upper bound over canonical typed request arguments and all scalar fact values; local no-embedding acquisition; excludes separately budgeted model framing").map_err(|_| SessionError::InvalidInput)?,
        },
    }.budget_bound().map_err(|_| SessionError::OutcomeUnknown)
}
impl ScalarQuotes {
    fn check(
        &self,
        allocation: &SessionResourceAllocation,
        target: &ConnectorTarget,
    ) -> Result<(), SessionError> {
        if !std::ptr::eq(allocation, self.allocation.allocation()) {
            return Err(SessionError::AuthorityDenied);
        }
        if target != &self.target || allocation.binding().scope != target.scope {
            return Err(SessionError::ScopeDenied);
        }
        if allocation.cost_unit() != &self.tariff.unit {
            return Err(SessionError::InvalidInput);
        }
        Ok(())
    }
    fn request<'a>(
        &'a self,
        allocation: &SessionResourceAllocation,
        request: &ConnectorRequest,
        source: Option<&SourceId>,
    ) -> Result<&'a CanonicalResultNormalizer, SessionError> {
        self.check(allocation, &request.target)?;
        if request.step != allocation.binding().step {
            return Err(SessionError::AuthorityDenied);
        }
        let mapping = self
            .document
            .records()
            .iter()
            .find(|r| {
                r.capability == request.capability && r.class.operation_class() == request.class
            })
            .ok_or(SessionError::UnsupportedCapability)?;
        if source.is_some() && source != mapping.source.as_ref() {
            return Err(SessionError::AuthorityDenied);
        }
        self.normalizers
            .get(&request.capability)
            .ok_or(SessionError::UnsupportedCapability)
    }
    fn quote(
        &self,
        results: u64,
        bytes: u64,
        cost: u64,
    ) -> Result<SessionConnectorQuote, SessionError> {
        Ok(SessionConnectorQuote {
            cost_unit: self.tariff.unit.clone(),
            amounts: SessionResourceAmounts {
                connector_calls: 1,
                retrieval_rounds: 1,
                retrieval_results: results,
                retrieval_tokens: token_bound(bytes)?,
                cost_microunits: cost,
                ..Default::default()
            },
        })
    }
    fn inventory_bound(&self) -> Result<(u64, u64), SessionError> {
        let digest = "0".repeat(64);
        let capabilities: Vec<_> = self.document.records().iter().map(|r| json!({"capability":r.capability.as_str(),"class":format!("{:?}",r.class.operation_class()),"argument_schema_digest":digest,"result_schema_digest":digest})).collect();
        let resources: Vec<_> = self
            .document
            .records()
            .iter()
            .filter_map(|r| r.source.as_ref().map(SourceId::as_str))
            .collect();
        let count = (capabilities.len() + resources.len()) as u64;
        let bytes = serde_json::to_vec(&json!({"target":{"connector":self.target.connector.as_str(),"scope":self.target.scope.as_str()},"revision":digest,"capabilities":capabilities,"resources":resources})).map_err(|_| SessionError::InvalidInput)?.len() as u64;
        // Empty discovery arguments contribute two canonical JSON bytes.
        Ok((
            count,
            bytes.checked_add(2).ok_or(SessionError::LimitExceeded)?,
        ))
    }
}
impl SessionConnectorQuotePort for ScalarQuotes {
    fn quote_discovery(
        &self,
        a: &SessionResourceAllocation,
        t: &ConnectorTarget,
    ) -> Result<SessionConnectorQuote, SessionError> {
        self.check(a, t)?;
        let (results, bytes) = self.inventory_bound()?;
        self.quote(results, bytes, self.tariff.discovery)
    }
    fn quote_request(
        &self,
        a: &SessionResourceAllocation,
        r: &ConnectorRequest,
        s: Option<&SourceId>,
    ) -> Result<SessionConnectorQuote, SessionError> {
        let bounds = self
            .request(a, r, s)?
            .resource_bounds()
            .map_err(|_| SessionError::UnsupportedCapability)?;
        self.quote(
            bounds.subjects().len() as u64,
            argument_bytes(r)?
                .checked_add(bounds.value_bytes())
                .ok_or(SessionError::LimitExceeded)?,
            self.tariff.operation,
        )
    }
    fn validate_inventory(
        &self,
        a: &SessionResourceAllocation,
        q: &SessionConnectorQuote,
        i: &ConnectorInventory,
    ) -> Result<(), SessionError> {
        if self.quote_discovery(a, &i.target)? != *q {
            return Err(SessionError::OutcomeUnknown);
        }
        let unique: std::collections::BTreeSet<_> = i
            .capabilities
            .iter()
            .map(|c| c.capability.clone())
            .collect();
        if unique.len() != self.document.records().len()
            || i.capabilities.len() != self.document.records().len()
            || i.capabilities.iter().any(|c| {
                !self
                    .document
                    .records()
                    .iter()
                    .any(|r| r.capability == c.capability && r.class.operation_class() == c.class)
            })
        {
            return Err(SessionError::OutcomeUnknown);
        }
        let mut sources: Vec<_> = self
            .document
            .records()
            .iter()
            .filter_map(|r| r.source.clone())
            .collect();
        sources.sort();
        if i.resources != sources {
            return Err(SessionError::OutcomeUnknown);
        }
        Ok(())
    }
    fn validate_observation(
        &self,
        a: &SessionResourceAllocation,
        q: &SessionConnectorQuote,
        r: &ConnectorRequest,
        s: Option<&SourceId>,
        o: &ConnectorObservation,
    ) -> Result<(), SessionError> {
        let bounds = self
            .request(a, r, s)?
            .resource_bounds()
            .map_err(|_| SessionError::UnsupportedCapability)?;
        if self.quote_request(a, r, s)? != *q
            || &o.source != bounds.source()
            || o.target != r.target
            || o.inventory_revision != r.inventory_revision
            || o.correlation != r.correlation
            || o.records.facts().len() != bounds.subjects().len()
            || o.records
                .facts()
                .iter()
                .map(|f| f.subject().clone())
                .collect::<std::collections::BTreeSet<_>>()
                != *bounds.subjects()
        {
            return Err(SessionError::OutcomeUnknown);
        }
        let output = value_bytes(o.records.facts().iter().map(Fact::value))?;
        if o.records.facts().iter().any(|f| {
            !matches!(
                f.value(),
                TypedValue::Boolean(_) | TypedValue::Integer(_) | TypedValue::String(_)
            )
        }) || output > bounds.value_bytes()
            || argument_bytes(r)?
                .checked_add(output)
                .is_none_or(|n| n > q.amounts.retrieval_tokens)
        {
            return Err(SessionError::LimitExceeded);
        }
        Ok(())
    }
}

#[cfg(test)]
#[path = "scalar_quotes_tests.rs"]
pub(super) mod tests;
