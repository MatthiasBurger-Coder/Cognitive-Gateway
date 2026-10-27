//! Explicit operator reference aliases for current native declarations. An alias
//! has its own registered revision; it is neither a catalog revision nor a grant.
use gateway_application::sessions::SessionError;
use gateway_domain::{
    CapabilityDefinition, CapabilityId, ContextScopeId, ReferenceId, ResolvedReference,
    SchemaVersion, context_content_digest,
};
use serde_json::Value;

pub(super) fn validate(
    scope: &ContextScopeId,
    proposed: &ResolvedReference<CapabilityId>,
    registered: &ResolvedReference<ReferenceId>,
    document: &Value,
    current: &CapabilityDefinition,
) -> Result<(), SessionError> {
    if &proposed.scope != scope || &registered.scope != scope {
        return Err(SessionError::ScopeDenied);
    }
    if registered.contract.as_str() != "cg.connector-capability"
        || registered.contract_version != SchemaVersion::V1
    {
        return Err(SessionError::InvalidInput);
    }
    if proposed.id.as_str() != registered.id.as_str()
        || &proposed.id != current.id()
        || proposed.contract != registered.contract
        || proposed.contract_version != registered.contract_version
        || proposed.revision != registered.revision
        || proposed.digest != registered.digest
    {
        return Err(SessionError::StaleRevision);
    }
    let resolved: CapabilityDefinition =
        serde_json::from_value(document.clone()).map_err(|_| SessionError::InvalidInput)?;
    let expected = serde_json::to_value(current).map_err(|_| SessionError::InvalidInput)?;
    let bytes = serde_json::to_vec(document).map_err(|_| SessionError::InvalidInput)?;
    if resolved != *current
        || document != &expected
        || context_content_digest(&bytes) != registered.digest
    {
        return Err(SessionError::StaleRevision);
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use gateway_domain::{CapabilityClass, ContentDigest};

    fn fixture() -> (
        ContextScopeId,
        CapabilityDefinition,
        ResolvedReference<CapabilityId>,
        ResolvedReference<ReferenceId>,
        Value,
    ) {
        let scope = ContextScopeId::new("alias-scope").unwrap();
        let current = CapabilityDefinition::new_with_contract(
            CapabilityId::new("records.update").unwrap(),
            CapabilityClass::Mutate,
            "records",
            "Update the selected record",
            ["record"],
            ["record"],
            ["record.exists"],
            ["consent.required"],
            ["scoped"],
        )
        .unwrap();
        let document = serde_json::to_value(&current).unwrap();
        let registered = ResolvedReference {
            id: ReferenceId::new(current.id().as_str()).unwrap(),
            scope: scope.clone(),
            contract: ReferenceId::new("cg.connector-capability").unwrap(),
            contract_version: SchemaVersion::V1,
            revision: ReferenceId::new("operator-revision-7").unwrap(),
            digest: context_content_digest(&serde_json::to_vec(&document).unwrap()),
        };
        let proposed = ResolvedReference {
            id: current.id().clone(),
            scope: registered.scope.clone(),
            contract: registered.contract.clone(),
            contract_version: registered.contract_version,
            revision: registered.revision.clone(),
            digest: registered.digest.clone(),
        };
        (scope, current, proposed, registered, document)
    }

    #[test]
    fn explicit_operator_alias_resolves_only_the_current_native_declaration() {
        let (scope, current, proposed, registered, document) = fixture();
        assert_eq!(
            validate(&scope, &proposed, &registered, &document, &current),
            Ok(())
        );
        // Revisions belong to explicit operator registrations, not a guessed
        // catalog or inventory version. A newly registered alias is admissible
        // only when the proposed reference names that exact new revision.
        let mut rotated = registered.clone();
        rotated.revision = ReferenceId::new("operator-revision-8").unwrap();
        let mut selected = proposed.clone();
        selected.revision = rotated.revision.clone();
        assert_eq!(
            validate(&scope, &selected, &rotated, &document, &current),
            Ok(())
        );
        let changed = current
            .clone()
            .with_description("Changed native meaning")
            .unwrap();
        assert_eq!(
            validate(&scope, &proposed, &registered, &document, &changed),
            Err(SessionError::StaleRevision)
        );
        let mut wrong = document.clone();
        wrong["class"] = Value::String("INSPECT".into());
        assert_eq!(
            validate(&scope, &proposed, &registered, &wrong, &current),
            Err(SessionError::StaleRevision)
        );
        wrong = document.clone();
        wrong["unknown"] = Value::Bool(true);
        assert_eq!(
            validate(&scope, &proposed, &registered, &wrong, &current),
            Err(SessionError::InvalidInput)
        );
    }

    #[test]
    fn every_alias_pin_field_and_actual_content_are_required() {
        let (scope, current, proposed, registered, document) = fixture();
        for field in [
            "id",
            "scope",
            "contract",
            "contract_version",
            "revision",
            "digest",
        ] {
            let mut changed = proposed.clone();
            match field {
                "id" => changed.id = CapabilityId::new("other.record").unwrap(),
                "scope" => changed.scope = ContextScopeId::new("other-scope").unwrap(),
                "contract" => changed.contract = ReferenceId::new("other-contract").unwrap(),
                "contract_version" => changed.contract_version = SchemaVersion::V2,
                "revision" => changed.revision = ReferenceId::new("operator-revision-8").unwrap(),
                "digest" => changed.digest = ContentDigest::new("f".repeat(64)).unwrap(),
                _ => unreachable!(),
            }
            assert!(
                validate(&scope, &changed, &registered, &document, &current).is_err(),
                "{field}"
            );
        }
        let mut changed = registered.clone();
        changed.contract = ReferenceId::new("fixture-capability").unwrap();
        let mut claimed = proposed.clone();
        claimed.contract = changed.contract.clone();
        assert_eq!(
            validate(&scope, &claimed, &changed, &document, &current),
            Err(SessionError::InvalidInput)
        );
        changed = registered.clone();
        changed.contract_version = SchemaVersion::V2;
        claimed = proposed.clone();
        claimed.contract_version = changed.contract_version;
        assert_eq!(
            validate(&scope, &claimed, &changed, &document, &current),
            Err(SessionError::InvalidInput)
        );
        changed = registered.clone();
        changed.digest = ContentDigest::new("e".repeat(64)).unwrap();
        claimed = proposed.clone();
        claimed.digest = changed.digest.clone();
        assert_eq!(
            validate(&scope, &claimed, &changed, &document, &current),
            Err(SessionError::StaleRevision)
        );
    }
}
