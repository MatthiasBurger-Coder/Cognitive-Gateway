//! Shared strict record mapping for CLI and admitted local hosts. No subprocesses.
#[cfg(test)]
pub(crate) mod native_producer_fixture;

use super::{inputs::*, *};
use gateway_application::{
    codex::{CompileCommand, FacadeError},
    policy_application::PolicyContext,
    resolution_application::{DeclarativeResolutionApplication, ResolvedPlan},
    resolution_composition::CompositionRules,
    resolution_snapshot::ResolutionSnapshotInput,
};
use gateway_policy::PolicyAuthority;
use std::path::Path;

fn failure(error: CliError) -> FacadeError {
    match error.exit {
        7 | 9 => FacadeError::StaleRevision,
        8 => FacadeError::PolicyDenied,
        _ => FacadeError::InvalidInput,
    }
}
pub(crate) fn capture(
    catalog: &Path,
    documents: &[Value],
) -> Result<(ResolutionSnapshotInput, CompositionRules), FacadeError> {
    if documents.len() != 3 {
        return Err(FacadeError::InvalidInput);
    }
    let options = Options {
        command: "resolve".into(),
        values: [
            (
                "catalog".into(),
                catalog.to_str().ok_or(FacadeError::InvalidInput)?.into(),
            ),
            ("plan".into(), documents[0].to_string()),
            ("rules".into(), documents[1].to_string()),
            ("process".into(), documents[2].to_string()),
        ]
        .into(),
        json: true,
    };
    let (input, rules, _) = pipeline::capture_resolution(&options).map_err(failure)?;
    Ok((input, rules))
}
/// Map bounded operator policy input against the exact current resolution.
/// This is the shared canonical authority assembly; it requires no context
/// projection and grants no connector effects by itself. Callers still use
/// `PolicyApplication` to validate and evaluate the resulting step context.
pub(crate) fn policy(
    resolved: &ResolvedPlan,
    document: Value,
) -> Result<(PolicyAuthority, PolicyContext, Value), FacadeError> {
    let input: PolicyInput = decode(document).map_err(failure)?;
    version(input.schema_version).map_err(failure)?;
    let basis = DeclarativeResolutionApplication.inspect_basis(resolved)?;
    if input.basis != basis {
        return Err(FacadeError::StaleRevision);
    }
    let authority = PolicyAuthority {
        policies: input
            .policies
            .into_iter()
            .map(PolicyDefinitionInput::build)
            .collect::<Result<_, _>>()
            .map_err(failure)?,
        capabilities: resolved
            .snapshot
            .input()
            .index
            .entries()
            .map(|entry| (entry.id().clone(), entry.capability().clone()))
            .collect(),
        constraints: input.constraints,
        required_evidence: input.required_evidence,
    };
    let context = PolicyContext {
        basis: resolved.report.basis.clone(),
        operating_mode: input.operating_mode,
        execution_profile: input.execution_profile,
        steps: input
            .steps
            .into_iter()
            .map(|(id, facts)| (id, facts.build()))
            .collect(),
    };
    Ok((authority, context, basis))
}

pub(crate) fn compile(
    resolved: ResolvedPlan,
    policy_document: Value,
    projection: Value,
) -> Result<CompileCommand, FacadeError> {
    let (authority, context, basis) = policy(&resolved, policy_document)?;
    pipeline::map_compile(projection, resolved, authority, context, &basis).map_err(failure)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn canonical_fixture() -> (ResolvedPlan, Value) {
        let fixture =
            Path::new(env!("CARGO_MANIFEST_DIR")).join("../../tests/fixtures/declarative-cli");
        let document = |name: &str| -> Value {
            serde_json::from_str(
                &std::fs::read_to_string(fixture.join(format!("{name}.json"))).unwrap(),
            )
            .unwrap()
        };
        let options = Options {
            command: "plan".into(),
            values: [
                (
                    "catalog".into(),
                    fixture.join("catalog").to_str().unwrap().into(),
                ),
                ("context".into(), document("context").to_string()),
                ("intent".into(), document("intent").to_string()),
                ("rules".into(), document("rules").to_string()),
            ]
            .into(),
            json: true,
        };
        let (plan, exit) = pipeline::execute(&options).unwrap();
        assert_eq!(exit, 0);
        let (snapshot, rules) = capture(
            &fixture.join("catalog"),
            &[plan, document("rules"), document("process")],
        )
        .unwrap();
        let resolved =
            gateway_application::resolution_application::DeclarativeResolutionApplication
                .resolve_plan(&snapshot, &rules)
                .unwrap();
        (resolved, document("policy"))
    }

    #[test]
    fn canonical_policy_maps_actual_authority_and_step_facts_without_projection() {
        use gateway_domain::{CapabilityClass, CapabilityId, PlanStepId, PolicyId};
        use gateway_policy::Approval;
        let (resolved, document) = canonical_fixture();
        let (authority, context, basis) = policy(&resolved, document.clone()).unwrap();
        let capability = CapabilityId::new("architecture.dependency-analysis").unwrap();
        assert_eq!(authority.policies.len(), 1);
        assert_eq!(
            authority.policies[0].id(),
            &PolicyId::new("inspect-policy").unwrap()
        );
        assert!(
            authority.policies[0]
                .allowed_capability_ids()
                .contains(&capability)
        );
        assert_eq!(
            authority.capabilities[&capability].class(),
            CapabilityClass::Inspect
        );
        assert_eq!(context.basis, resolved.report.basis);
        assert_eq!(basis, document["basis"]);
        assert_eq!(
            context.operating_mode,
            resolved.snapshot.input().operating_mode
        );
        assert_eq!(
            context.execution_profile,
            resolved.snapshot.input().execution_profile
        );
        let facts = &context.steps[&PlanStepId::new("step-condition.0").unwrap()];
        assert_eq!(facts.authorizations[&capability], Approval::Granted);
        assert!(facts.consents.is_empty());
        assert!(facts.evidence.contains("repository.available"));
        assert!(facts.satisfied_constraints.contains("read-only"));
        assert!(facts.prerequisites_satisfied);
        let report = gateway_application::policy_application::PolicyApplication
            .evaluate(&resolved, &authority, &context)
            .unwrap();
        assert_eq!(
            report.steps()[&PlanStepId::new("step-condition.0").unwrap()].decision,
            gateway_policy::PolicyDecision::Allow,
        );
    }

    #[test]
    fn canonical_policy_rejects_stale_basis_schema_and_unknown_step_facts() {
        let (resolved, document) = canonical_fixture();
        let mut stale = document.clone();
        stale["basis"]["scope"] = json!("another-project");
        assert!(matches!(
            policy(&resolved, stale),
            Err(FacadeError::StaleRevision)
        ));
        let mut unsupported_schema = document.clone();
        unsupported_schema["schema_version"] = json!(2);
        assert!(matches!(
            policy(&resolved, unsupported_schema),
            Err(FacadeError::InvalidInput)
        ));
        let mut unknown_fact = document;
        unknown_fact["steps"]["step-condition.0"]["connector_granted"] = json!(true);
        assert!(matches!(
            policy(&resolved, unknown_fact),
            Err(FacadeError::InvalidInput)
        ));
    }

    #[test]
    fn incomplete_snapshot_records_are_refused_before_catalog_access() {
        assert!(matches!(
            capture(Path::new("/missing-catalog"), &[]),
            Err(FacadeError::InvalidInput)
        ));
        assert!(matches!(
            capture(
                Path::new("/missing-catalog"),
                &[json!({}), json!({}), json!({})]
            ),
            Err(FacadeError::InvalidInput)
        ));
    }
}
