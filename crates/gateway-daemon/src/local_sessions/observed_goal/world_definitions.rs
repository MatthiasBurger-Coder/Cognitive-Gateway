//! Exact externally pinned complete-world DATA definitions. A declaration is
//! not evidence that a provider implements an atomic export; #408 qualifies the
//! actual transport/profile separately. These types grant no operation.
use super::definitions::{NormalizationDocument, Pin, ReadAdmission, ReadonlyDefinitions};
use gateway_application::{
    connectors::*,
    sessions::{SessionError, scoped_snapshots::*},
};
use gateway_domain::*;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::{BTreeMap, BTreeSet};

#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(in crate::local_sessions) struct AtomicBeforeAdmission {
    pub universe: Pin,
    pub profile: Pin,
    pub provider: Pin,
    pub read: ReadAdmission,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(in crate::local_sessions) struct AtomicProviderDefinition {
    pub schema_version: SchemaVersion,
    pub scope: ContextScopeId,
    pub task: Pin,
    pub projection: Pin,
    pub mapping: Pin,
    pub universe: Pin,
    pub profile: Pin,
    pub route: Pin,
    pub scope_target: Pin,
    pub objects: Vec<Pin>,
    pub selector_arguments: BTreeMap<String, String>,
    pub response: AtomicResponse,
    pub semantics: AtomicExportSemantics,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(in crate::local_sessions) struct AtomicResponse {
    pub object_field: String,
    pub identity_fields: BTreeMap<String, String>,
}
#[derive(Deserialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub(in crate::local_sessions) enum AtomicExportSemantics {
    AtomicCompleteExportV1,
}
pub(in crate::local_sessions) struct AtomicBeforeDefinitions {
    pub universe: SnapshotUniverse,
    pub profile: SnapshotProfile,
    pub provider: AtomicProviderDefinition,
    pub read: ReadAdmission,
    pub normalization: NormalizationDocument,
}
fn invalid() -> SessionError {
    SessionError::InvalidInput
}
fn stale() -> SessionError {
    SessionError::StaleRevision
}
fn pin(pin: &Pin, scope: &ContextScopeId, contract: Option<&str>) -> Result<(), SessionError> {
    if &pin.scope != scope
        || pin.contract_version != SchemaVersion::V1
        || contract.is_some_and(|kind| pin.contract.as_str() != kind)
        || gateway_domain::model_invocation::validate_model_binding(pin).is_err()
    {
        return Err(invalid());
    }
    Ok(())
}
fn bytes(value: &impl Serialize) -> Result<Vec<u8>, SessionError> {
    serde_json::to_vec(&serde_json::to_value(value).map_err(|_| invalid())?).map_err(|_| invalid())
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Header {
    id: ReferenceId,
    scope: ContextScopeId,
    contract: ReferenceId,
    contract_version: SchemaVersion,
    revision: ReferenceId,
}
impl Header {
    fn check(&self, actual: &Pin) -> Result<(), SessionError> {
        if self.id != actual.id
            || self.scope != actual.scope
            || self.contract != actual.contract
            || self.contract_version != actual.contract_version
            || self.revision != actual.revision
        {
            return Err(stale());
        }
        Ok(())
    }
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct RouteBody {
    definition: Header,
    authority_domain: Pin,
    target: Target,
    capability: CapabilityId,
    class: Class,
    arguments: BTreeMap<NonEmptyText, TypedValue>,
    object_reference: NonEmptyText,
    epoch_argument: Option<NonEmptyText>,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Target {
    connector: NonEmptyText,
    scope: ContextScopeId,
}
#[derive(Deserialize)]
#[serde(rename_all = "lowercase")]
enum Class {
    Read,
    Search,
    Inspect,
    Mutate,
    Admin,
}
impl Class {
    fn native(self) -> ConnectorOperationClass {
        match self {
            Self::Read => ConnectorOperationClass::Read,
            Self::Search => ConnectorOperationClass::Search,
            Self::Inspect => ConnectorOperationClass::Inspect,
            Self::Mutate => ConnectorOperationClass::Mutate,
            Self::Admin => ConnectorOperationClass::Admin,
        }
    }
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Route {
    pin: Pin,
    body: Value,
}
impl Route {
    fn native(
        self,
        docs: &BTreeMap<ReferenceId, (Pin, Value)>,
        scope: &ContextScopeId,
    ) -> Result<SnapshotReadRoute, SessionError> {
        let body: RouteBody = serde_json::from_value(self.body.clone()).map_err(|_| invalid())?;
        pin(&self.pin, scope, Some("cg.scoped-snapshot-read-route"))?;
        body.definition.check(&self.pin)?;
        if document(docs, &self.pin)? != &self.body {
            return Err(stale());
        }
        pin(&body.authority_domain, scope, None)?;
        document(docs, &body.authority_domain)?;
        let route = SnapshotReadRoute {
            pin: self.pin,
            authority_domain: body.authority_domain,
            target: ConnectorTarget {
                connector: body.target.connector,
                scope: body.target.scope,
            },
            capability: body.capability,
            class: body.class.native(),
            arguments: body.arguments,
            object_reference: body.object_reference,
            epoch_argument: body.epoch_argument,
        };
        if route.target.scope != *scope
            || route.arguments.len() > 256
            || !matches!(
                route.class,
                ConnectorOperationClass::Read
                    | ConnectorOperationClass::Search
                    | ConnectorOperationClass::Inspect
            )
            || route.pin.digest
                != context_content_digest(&route.body_bytes().map_err(|_| invalid())?)
            || bytes(&self.body)? != route.body_bytes().map_err(|_| invalid())?
        {
            return Err(stale());
        }
        Ok(route)
    }
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Member {
    object: Pin,
    source: SourceId,
    source_kind: SourceKind,
    object_reference: NonEmptyText,
    subjects: Vec<Vec<NonEmptyText>>,
    version_subject: Vec<NonEmptyText>,
    route: Route,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Universe {
    definition: Header,
    objects: Vec<Member>,
}
fn subject(parts: Vec<NonEmptyText>) -> Result<SubjectPath, SessionError> {
    SubjectPath::new(parts.iter().map(NonEmptyText::as_str)).map_err(|_| invalid())
}
fn document<'a>(
    docs: &'a BTreeMap<ReferenceId, (Pin, Value)>,
    expected: &Pin,
) -> Result<&'a Value, SessionError> {
    let (actual, body) = docs.get(&expected.id).ok_or(SessionError::Unavailable)?;
    if actual != expected {
        return Err(stale());
    }
    Ok(body)
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Profile {
    definition: Header,
    authority_domain: Pin,
    kind: Kind,
    anchor_source: SourceId,
    anchor_source_kind: SourceKind,
    anchor_route: Route,
    epoch_subject: Vec<NonEmptyText>,
    membership_subject: Vec<NonEmptyText>,
    max_age_seconds: u64,
}
#[derive(Deserialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
enum Kind {
    AtomicCompleteExport,
    SnapshotScopedReads,
    BracketedNonReusedAnchor,
}
impl AtomicBeforeAdmission {
    pub fn roots(&self) -> Vec<(&Pin, &'static str)> {
        vec![
            (&self.universe, "cg.scoped-snapshot-universe"),
            (&self.profile, "cg.scoped-snapshot-profile"),
            (&self.provider, "cg.connector-atomic-export-provider"),
            (&self.read.normalization, "cg.connector-normalization"),
        ]
    }
    pub fn capture(
        &self,
        defs: &ReadonlyDefinitions,
    ) -> Result<AtomicBeforeDefinitions, SessionError> {
        let docs = &defs.documents;
        let scope = &defs.runtime.scope;
        let task_pin = &defs.runtime.task;
        let projection = &defs.runtime.projection;
        let mapping = &defs.runtime.mapping;
        let access_profile = &defs.runtime.access_profile;
        let task = &defs.task;
        let connector = &defs.runtime.connector;
        for (root, contract) in self.roots() {
            pin(root, scope, Some(contract))?;
            document(docs, root)?;
        }
        let raw = document(docs, &self.universe)?;
        let declared: Universe = serde_json::from_value(raw.clone()).map_err(|_| invalid())?;
        declared.definition.check(&self.universe)?;
        if declared.objects.is_empty() || declared.objects.len() > 256 {
            return Err(SessionError::LimitExceeded);
        }
        let mut objects = Vec::new();
        let mut domains = BTreeSet::new();
        let mut owners = BTreeMap::new();
        for member in declared.objects {
            pin(&member.object, scope, Some("cg.connector-object"))?;
            document(docs, &member.object)?;
            let mut subjects = BTreeSet::new();
            for s in member.subjects {
                if !subjects.insert(subject(s)?) {
                    return Err(invalid());
                }
            }
            let version_subject = subject(member.version_subject)?;
            if subjects.is_empty()
                || !subjects.contains(&version_subject)
                || member.source_kind != SourceKind::Tool
                || !domains.insert((
                    member.source_kind,
                    member.source.clone(),
                    bytes(&member.object)?,
                ))
            {
                return Err(invalid());
            }
            for s in &subjects {
                if owners
                    .insert(s.clone(), member.object.clone())
                    .is_some_and(|old| old != member.object)
                {
                    return Err(invalid());
                }
            }
            objects.push(SnapshotObject {
                object: member.object,
                source: member.source,
                source_kind: member.source_kind,
                object_reference: member.object_reference,
                subjects,
                version_subject,
                route: member.route.native(docs, scope)?,
            });
        }
        let universe = SnapshotUniverse {
            pin: self.universe.clone(),
            objects,
        };
        if universe.body_bytes().map_err(|_| invalid())? != bytes(raw)? {
            return Err(stale());
        }
        let raw_profile = document(docs, &self.profile)?;
        let declared: Profile =
            serde_json::from_value(raw_profile.clone()).map_err(|_| invalid())?;
        declared.definition.check(&self.profile)?;
        if !matches!(declared.kind, Kind::AtomicCompleteExport) {
            return Err(SessionError::UnsupportedCapability);
        }
        pin(&declared.authority_domain, scope, None)?;
        document(docs, &declared.authority_domain)?;
        let profile = SnapshotProfile {
            pin: self.profile.clone(),
            authority_domain: declared.authority_domain,
            kind: SnapshotProfileKind::AtomicCompleteExport,
            anchor_source: declared.anchor_source,
            anchor_source_kind: declared.anchor_source_kind,
            anchor_route: declared.anchor_route.native(docs, scope)?,
            epoch_subject: subject(declared.epoch_subject)?,
            membership_subject: subject(declared.membership_subject)?,
            max_age_seconds: declared.max_age_seconds,
        };
        if profile.body_bytes().map_err(|_| invalid())? != bytes(raw_profile)?
            || profile.anchor_source_kind != SourceKind::Tool
            || profile.max_age_seconds == 0
            || profile.max_age_seconds > 3600
            || profile.anchor_route.epoch_argument.is_some()
            || profile.anchor_route.authority_domain != profile.authority_domain
            || profile.anchor_route.target.connector != *connector
            || profile.epoch_subject == profile.membership_subject
            || owners.contains_key(&profile.epoch_subject)
            || owners.contains_key(&profile.membership_subject)
            || universe.objects.iter().any(|o| {
                o.route != profile.anchor_route
                    || o.source != profile.anchor_source
                    || o.route.authority_domain != profile.authority_domain
            })
        {
            return Err(invalid());
        }
        let provider: AtomicProviderDefinition =
            serde_json::from_value(document(docs, &self.provider)?.clone())
                .map_err(|_| invalid())?;
        if provider.schema_version != SchemaVersion::V1
            || provider.scope != *scope
            || provider.task != *task_pin
            || provider.projection != *projection
            || provider.mapping != *mapping
            || provider.universe != self.universe
            || provider.profile != self.profile
            || provider.route != profile.anchor_route.pin
            || provider.scope_target != task.data().target.reference
            || provider.objects.is_empty()
            || provider.objects.len() > 256
            || provider.selector_arguments.is_empty()
            || provider.selector_arguments.len() > 64
            || provider.response.identity_fields.is_empty()
            || provider.response.identity_fields.len() > 64
            || provider.response.object_field.is_empty()
        {
            return Err(stale());
        }
        pin(&provider.scope_target, scope, Some("cg.connector-object"))?;
        document(docs, &provider.scope_target)?;
        let expected = universe
            .objects
            .iter()
            .map(|m| bytes(&m.object))
            .collect::<Result<BTreeSet<_>, _>>()?;
        let members = provider
            .objects
            .iter()
            .map(bytes)
            .collect::<Result<BTreeSet<_>, _>>()?;
        if members.len() != provider.objects.len() || members != expected {
            return Err(invalid());
        }
        for member in &provider.objects {
            document(docs, member)?;
            if member != &provider.scope_target
                && !task
                    .data()
                    .inputs
                    .iter()
                    .any(|i| matches!(&i.value, SemanticInputValue::Reference(p) if p == member))
            {
                return Err(SessionError::AuthorityDenied);
            }
        }
        let normalization: NormalizationDocument =
            serde_json::from_value(document(docs, &self.read.normalization)?.clone())
                .map_err(|_| invalid())?;
        let super::definitions::ReadSemantics::DirectCurrentReadProfile {
            max_age_seconds, ..
        } = normalization.acquisition.semantics;
        let mut expected_subjects = owners.into_keys().collect::<BTreeSet<_>>();
        expected_subjects.extend([
            profile.epoch_subject.clone(),
            profile.membership_subject.clone(),
        ]);
        if expected_subjects.len() > 256
            || normalization.fields.len() != expected_subjects.len()
            || normalization
                .fields
                .values()
                .cloned()
                .collect::<BTreeSet<_>>()
                != expected_subjects
            || normalization.source != self.read.source
            || normalization.source != profile.anchor_source
            || normalization.acquisition.source_kind != SourceKind::Tool
            || normalization.acquisition.authenticated_profile != *access_profile
            || normalization.revision_field.is_none()
            || normalization.schema_version != SchemaVersion::V1
            || normalization.object_field != provider.response.object_field
            || max_age_seconds != profile.max_age_seconds
            || self.read.capability != profile.anchor_route.capability
            || self.read.arguments != profile.anchor_route.arguments
        {
            return Err(invalid());
        }
        if universe
            .body_bytes()
            .map_err(|_| invalid())?
            .len()
            .checked_add(profile.body_bytes().map_err(|_| invalid())?.len())
            .is_none_or(|n| n >= gateway_application::sessions::MAX_BEFORE_RECORD_BYTES)
        {
            return Err(SessionError::LimitExceeded);
        }
        normalization.profile()?;
        Ok(AtomicBeforeDefinitions {
            universe,
            profile,
            provider,
            read: self.read.clone(),
            normalization,
        })
    }
}

#[cfg(test)]
#[path = "world_definition_tests.rs"]
mod tests;
