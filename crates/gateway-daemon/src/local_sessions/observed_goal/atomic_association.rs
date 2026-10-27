//! Native complete-world association; no goal conditions or legacy export aliases.
use super::{definitions::*, read_association::*, world_definitions::*};
use crate::mcp_client::{
    ProtocolResult,
    mapping::{MappedInventory, MappedToolSchema},
};
use gateway_application::{
    connector_manifest::InventoryKind, connectors::*, sessions::SessionError,
};
use gateway_domain::*;
use std::collections::{BTreeMap, BTreeSet};

pub(super) struct BoundAtomicRead {
    read: ReadAdmission,
    target: ConnectorTarget,
    revision: ContentDigest,
    task: TaskId,
    step: PlanStepId,
    execution: NonEmptyText,
    object: NonEmptyText,
    object_field: String,
    identities: BTreeMap<String, TypedValue>,
}
impl BoundAtomicRead {
    pub(super) fn admit(
        defs: &ReadonlyDefinitions,
        world: &AtomicBeforeDefinitions,
        inventory: &MappedInventory,
        schema: &MappedToolSchema,
        execution: NonEmptyText,
    ) -> Result<Self, SessionError> {
        let deny = || SessionError::ScopeDenied;
        let record = defs
            .mapping
            .records()
            .iter()
            .find(|r| r.reference == world.read.mapping_reference)
            .ok_or_else(deny)?;
        let entry = inventory
            .entries()
            .iter()
            .find(|e| e.mapping.reference == record.reference)
            .ok_or_else(deny)?;
        let descriptor = |pin: &Pin| {
            ScopeObjectDocument::from_json(
                pin,
                &serde_json::to_vec(defs.document(pin)?).map_err(|_| SessionError::InvalidInput)?,
            )
        };
        let root = descriptor(&world.provider.scope_target)?;
        let expected = world
            .universe
            .objects
            .iter()
            .flat_map(|o| o.subjects.iter().cloned())
            .collect::<BTreeSet<_>>();
        if world.provider.scope_target != defs.task.data().target.reference
            || root.kind() != &defs.task.data().target.kind
            || root.parent().is_some()
            || root.connector() != &defs.runtime.connector
            || root.subjects().iter().cloned().collect::<BTreeSet<_>>() != expected
            || root.object_reference() != &world.profile.anchor_route.object_reference
            || record.kind != InventoryKind::Tool
            || record.capability != world.read.capability
            || record.source.as_ref() != Some(&world.read.source)
            || record.class.operation_class() != world.profile.anchor_route.class
            || !matches!(
                record.class.operation_class(),
                ConnectorOperationClass::Read
                    | ConnectorOperationClass::Inspect
                    | ConnectorOperationClass::Search
            )
            || inventory.inventory().target != world.profile.anchor_route.target
        {
            return Err(deny());
        }
        for member in &world.universe.objects {
            let object = descriptor(&member.object)?;
            if object.parent() != Some(&world.provider.scope_target)
                || object.connector() != root.connector()
                || object.object_reference() != &member.object_reference
                || object.subjects().iter().cloned().collect::<BTreeSet<_>>() != member.subjects
            {
                return Err(deny());
            }
        }
        let used = world
            .provider
            .selector_arguments
            .values()
            .collect::<BTreeSet<_>>();
        if used != root.coordinates().keys().collect()
            || used.is_empty()
            || !used.is_subset(&world.provider.response.identity_fields.values().collect())
            || world
                .provider
                .response
                .identity_fields
                .contains_key(&world.provider.response.object_field)
        {
            return Err(deny());
        }
        let args = world
            .provider
            .selector_arguments
            .iter()
            .map(|(arg, key)| {
                Ok((
                    NonEmptyText::new(arg).map_err(|_| SessionError::InvalidInput)?,
                    root.coordinates().get(key).ok_or_else(deny)?.clone(),
                ))
            })
            .collect::<Result<BTreeMap<_, _>, SessionError>>()?;
        if args != world.read.arguments {
            return Err(deny());
        }
        if schema.target != inventory.inventory().target
            || schema.capability != record.capability
            || schema.inventory_revision != inventory.inventory().revision
            || schema.argument_schema_digest != entry.argument_schema_digest
            || schema.wire_argument_schema_digest != entry.argument_schema_digest
            || schema.result_schema_digest != entry.result_schema_digest
            || context_content_digest(
                &serde_json::to_vec(&schema.argument_schema)
                    .map_err(|_| SessionError::InvalidInput)?,
            ) != entry.argument_schema_digest
            || context_content_digest(
                &serde_json::to_vec(&schema.result_schema)
                    .map_err(|_| SessionError::InvalidInput)?,
            ) != entry.result_schema_digest
            || record.result_schema.as_ref() != Some(&schema.result_schema)
        {
            return Err(SessionError::StaleRevision);
        }
        let gateway_domain::semantic_output::OutputType::Object { fields } =
            gateway_application::prompt_rendering::parse_connector_input_schema(
                &schema.argument_schema,
            )
            .map_err(|_| SessionError::UnsupportedCapability)?
        else {
            return Err(SessionError::UnsupportedCapability);
        };
        if fields.len() != args.len()
            || !fields.iter().all(|(name, f)| {
                args.iter().any(|(key, value)| {
                    key.as_str() == name && scalar_matches(&f.value_type, value)
                })
            })
        {
            return Err(deny());
        }
        let identities = world
            .provider
            .response
            .identity_fields
            .iter()
            .map(|(field, key)| {
                Ok((
                    field.clone(),
                    root.coordinates().get(key).ok_or_else(deny)?.clone(),
                ))
            })
            .collect::<Result<BTreeMap<_, _>, SessionError>>()?;
        if !required(&schema.result_schema, &world.provider.response.object_field)
            || schema.result_schema["properties"][&world.provider.response.object_field]["type"]
                != "string"
            || identities.iter().any(|(key, value)| {
                !required(&schema.result_schema, key)
                    || !schema_scalar_matches(&schema.result_schema["properties"][key], value)
            })
            || world.normalization.fields.keys().any(|key| {
                !required(&schema.result_schema, key)
                    || !matches!(
                        schema.result_schema["properties"][key]["type"].as_str(),
                        Some("string" | "integer" | "boolean")
                    )
            })
        {
            return Err(SessionError::UnsupportedCapability);
        }
        let AtomicExportSemantics::AtomicCompleteExportV1 = world.provider.semantics;
        Ok(Self {
            read: world.read.clone(),
            target: inventory.inventory().target.clone(),
            revision: inventory.inventory().revision.clone(),
            task: defs.task.data().id.clone(),
            step: defs.native.step.clone(),
            execution,
            object: root.object_reference().clone(),
            object_field: world.provider.response.object_field.clone(),
            identities,
        })
    }
    pub(super) fn validate(
        &self,
        r: &ConnectorRequest,
        s: Option<&SourceId>,
    ) -> Result<(), SessionError> {
        if r.target != self.target
            || r.inventory_revision != self.revision
            || r.capability != self.read.capability
            || r.arguments != self.read.arguments
            || r.task != self.task
            || r.step != self.step
            || r.execution != self.execution
            || s.is_some()
            || !matches!(
                r.class,
                ConnectorOperationClass::Read
                    | ConnectorOperationClass::Inspect
                    | ConnectorOperationClass::Search
            )
        {
            Err(SessionError::ScopeDenied)
        } else {
            Ok(())
        }
    }
    pub(super) fn response(
        &self,
        r: &ConnectorRequest,
        result: &ProtocolResult,
    ) -> Result<(), ConnectorFailure> {
        let fail = || ConnectorFailure {
            code: ConnectorFailureCode::ScopeMismatch,
            effect: ConnectorEffect::NoMutation,
            correlation: r.correlation.clone(),
        };
        match result {
            ProtocolResult::Tool {
                structured: Some(value),
                is_error: false,
                ..
            } if value[&self.object_field].as_str() == Some(self.object.as_str())
                && self
                    .identities
                    .iter()
                    .all(|(k, v)| json_scalar_matches(&value[k], v)) =>
            {
                Ok(())
            }
            _ => Err(fail()),
        }
    }
}
