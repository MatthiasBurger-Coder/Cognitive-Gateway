//! Shared strict record mapping for CLI and admitted local hosts. No subprocesses.
use super::{inputs::*, *};
use gateway_application::{
    codex::{CompileCommand, FacadeError},
    policy_application::PolicyContext,
    resolution_application::ResolvedPlan,
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
pub(crate) fn compile(
    resolved: ResolvedPlan,
    policy: Value,
    projection: Value,
) -> Result<CompileCommand, FacadeError> {
    let input: PolicyInput = decode(policy).map_err(failure)?;
    version(input.schema_version).map_err(failure)?;
    let artifact: Value = serde_json::from_str(
        &gateway_application::resolution_application::DeclarativeResolutionApplication
            .serialize_resolution(&resolved, Default::default())?,
    )
    .map_err(|_| FacadeError::Internal)?;
    if input.basis != artifact["basis"] {
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
    pipeline::map_compile(projection, resolved, authority, context, &artifact["basis"])
        .map_err(failure)
}

#[cfg(test)]
mod tests {
    use super::*;

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
