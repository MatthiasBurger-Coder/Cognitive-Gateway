//! Owning definition identities exclude only their own content digest.
use super::{types::SnapshotData, *};
use gateway_domain::*;
use serde_json::{Value, json};

fn subject(value: &SubjectPath) -> Vec<&str> {
    value.segments().iter().map(NonEmptyText::as_str).collect()
}
fn own_pin(pin: &SnapshotPin) -> Value {
    json!({"id":pin.id,"scope":pin.scope,"contract":pin.contract,
        "contract_version":pin.contract_version,"revision":pin.revision})
}
fn bytes(value: Value) -> Result<Vec<u8>, SnapshotError> {
    serde_json::to_vec(&value).map_err(|_| SnapshotError::InvalidEncoding)
}
fn route_value(route: &SnapshotReadRoute) -> Value {
    json!({"definition":own_pin(&route.pin),"authority_domain":route.authority_domain,"target":{"connector":route.target.connector,"scope":route.target.scope},
        "capability":route.capability,"class":match route.class {
            crate::connectors::ConnectorOperationClass::Read=>"read",
            crate::connectors::ConnectorOperationClass::Search=>"search",
            crate::connectors::ConnectorOperationClass::Inspect=>"inspect",
            crate::connectors::ConnectorOperationClass::Mutate=>"mutate",
            crate::connectors::ConnectorOperationClass::Admin=>"admin"},
        "arguments":route.arguments,"object_reference":route.object_reference,"epoch_argument":route.epoch_argument})
}
fn member_value(object: &SnapshotObject) -> Value {
    json!({"object":object.object,"source":object.source,"source_kind":object.source_kind.as_str(),
        "object_reference":object.object_reference,"subjects":object.subjects.iter().map(subject).collect::<Vec<_>>(),
        "version_subject":subject(&object.version_subject),"route":{"pin":object.route.pin,"body":route_value(&object.route)}})
}
impl SnapshotReadRoute {
    pub fn body_bytes(&self) -> Result<Vec<u8>, SnapshotError> {
        bytes(route_value(self))
    }
}
impl SnapshotUniverse {
    pub fn body_bytes(&self) -> Result<Vec<u8>, SnapshotError> {
        bytes(
            json!({"definition":own_pin(&self.pin),"objects":self.objects.iter().map(member_value).collect::<Vec<_>>()}),
        )
    }
    /// Membership is an order-independent, complete source/object/subject set.
    /// Its provider observation is distinct from the owning document digest.
    pub fn membership_digest(&self) -> Result<ContentDigest, SnapshotError> {
        let mut members = self.objects.iter().map(|object| {
            (object.object.clone(), json!({"object":object.object,"source":object.source,
                "source_kind":object.source_kind.as_str(),"object_reference":object.object_reference,
                "subjects":object.subjects.iter().map(subject).collect::<Vec<_>>(),
                "version_subject":subject(&object.version_subject)}))
        }).collect::<Vec<_>>();
        members.sort_by_cached_key(|(_, value)| value.to_string());
        Ok(context_content_digest(&bytes(
            json!({"contract":"cg.scoped-snapshot-membership/1.0",
            "scope":self.pin.scope,"members":members.into_iter().map(|(_,value)|value).collect::<Vec<_>>()}),
        )?))
    }
}
impl SnapshotProfile {
    pub fn body_bytes(&self) -> Result<Vec<u8>, SnapshotError> {
        bytes(
            json!({"definition":own_pin(&self.pin),"authority_domain":self.authority_domain,
            "kind":match self.kind {SnapshotProfileKind::AtomicCompleteExport=>"ATOMIC_COMPLETE_EXPORT",
                SnapshotProfileKind::SnapshotScopedReads=>"SNAPSHOT_SCOPED_READS",
                SnapshotProfileKind::BracketedNonReusedAnchor=>"BRACKETED_NON_REUSED_ANCHOR"},
            "anchor_source":self.anchor_source,"anchor_source_kind":self.anchor_source_kind.as_str(),
            "anchor_route":{"pin":self.anchor_route.pin,"body":route_value(&self.anchor_route)},
            "epoch_subject":subject(&self.epoch_subject),"membership_subject":subject(&self.membership_subject),
            "max_age_seconds":self.max_age_seconds}),
        )
    }
}
impl SnapshotAcquisition {
    /// Hashes the complete actual request, observation and native batch grouping.
    pub fn reference(&self) -> Result<ContentDigest, SnapshotError> {
        Ok(context_content_digest(
            &super::codec::canonical_acquisition(self)?,
        ))
    }
}
pub(super) fn validate_identity(data: &SnapshotData) -> Result<(), SnapshotError> {
    let scope = &data.binding.capture.scope;
    let pin = |pin: &SnapshotPin, contract: Option<&str>| {
        pin.scope == *scope
            && pin.contract_version == SchemaVersion::V1
            && contract.is_none_or(|c| pin.contract.as_str() == c)
    };
    if data.binding.capture.validate().is_err() {
        return Err(SnapshotError::BindingMismatch);
    }
    if !pin(&data.universe.pin, Some("cg.scoped-snapshot-universe"))
        || data.universe.pin.digest != context_content_digest(&data.universe.body_bytes()?)
    {
        return Err(SnapshotError::UniverseMismatch);
    }
    if !pin(&data.profile.pin, Some("cg.scoped-snapshot-profile"))
        || !pin(&data.profile.authority_domain, None)
        || data.profile.pin.digest != context_content_digest(&data.profile.body_bytes()?)
    {
        return Err(SnapshotError::ProfileMismatch);
    }
    for route in data
        .universe
        .objects
        .iter()
        .map(|o| &o.route)
        .chain(std::iter::once(&data.profile.anchor_route))
    {
        if route.authority_domain != data.profile.authority_domain
            || !pin(&route.pin, Some("cg.scoped-snapshot-read-route"))
            || route.target.scope != *scope
            || route.pin.digest != context_content_digest(&route.body_bytes()?)
            || !matches!(
                route.class,
                crate::connectors::ConnectorOperationClass::Read
                    | crate::connectors::ConnectorOperationClass::Search
                    | crate::connectors::ConnectorOperationClass::Inspect
            )
            || route.arguments.len() > 256
        {
            return Err(SnapshotError::ProfileMismatch);
        }
    }
    if data.universe.objects.iter().any(|o| !pin(&o.object, None)) {
        return Err(SnapshotError::UniverseMismatch);
    }
    Ok(())
}
