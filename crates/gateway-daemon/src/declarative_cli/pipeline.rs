use super::inputs::Assessment;
use super::{inputs::*, *};
use gateway_application::{
    DeclarativePlanningApplication, DeclarativeSituationApplication, PlanningCapabilitySnapshot,
    PlanningRuleSnapshot, ProcessSnapshotInput,
    context_application::{CompileStepInput, ContextApplication, ContextProjection},
    policy_application::{PolicyApplication, PolicyContext},
    resolution::ResolutionOutcome,
    resolution_application::{
        DeclarativeResolutionApplication, ResolvedPlan, WorkflowProjectionMapping,
    },
    resolution_snapshot::ResolutionSnapshotInput,
};
use gateway_domain::*;
use gateway_policy::{PolicyAuthority, PolicyDecision};
use gateway_process::ProcessRegistry;
use gateway_registry::Registry;
use std::path::Path;

pub(super) fn execute(options: &Options) -> Result<(Value, i32), CliError> {
    match options.command.as_str() {
        "procedures" => super::promotion_cli::execute(options),
        "evaluate" | "simulate" | "replay" => super::procedure_cli::execute(options),
        "patterns" => {
            let report = if options.get("report").is_some() {
                decode(options.input("report")?)?
            } else {
                super::patterns_cli::inspect_database(options)?
            };
            Ok((value(&report)?, 0))
        }
        "assess" => Ok((value(&assess(options.input("context")?)?)?, 0)),
        "plan" => planning(options),
        "explain" if options.get("plan").is_none() => {
            if options.get("intent").is_some() {
                planning(options)
            } else {
                Ok((value(&assess(options.input("context")?)?)?, 0))
            }
        }
        _ => downstream(options),
    }
}
fn assess(input: Value) -> Result<Assessment, CliError> {
    if input.get("document").is_some() {
        let result: Assessment = decode(input)?;
        version(result.schema_version)?;
        return Ok(result);
    }
    let input: AssessmentInput = decode(input)?;
    version(input.schema_version)?;
    input
        .assess()
        .map_err(|error| CliError::new(4, "ASSESSMENT_FAILED", error.code()))
}
fn registry(options: &Options) -> Result<Registry, CliError> {
    checked(
        Registry::load_catalog(options.get("catalog").unwrap_or("catalog")),
        6,
        "CATALOG_ERROR",
    )
}
fn planning(options: &Options) -> Result<(Value, i32), CliError> {
    let mut assessment = assess(options.input("context")?)?;
    let intent: Intent = decode(options.input("intent")?)?;
    let desired = intent.desired_state().clone();
    if assessment
        .document
        .intent()
        .is_some_and(|old| old != &intent)
    {
        return Err(CliError::new(
            5,
            "INTENT_MISMATCH",
            "context and explicit intent disagree",
        ));
    }
    assessment.document = checked(
        DeclarativeSituationApplication::new().validate_declarative_context(
            assessment.document.context().clone(),
            Some(intent),
            assessment.document.records().cloned(),
            assessment.document.observed_state().clone(),
            assessment.document.situation().clone(),
        ),
        4,
        "ASSESSMENT_FAILED",
    )?;
    let rules: Rules = options.optional("rules")?;
    version(rules.schema_version)?;
    let capability_rules = rules.planning.build();
    let registry = registry(options)?;
    let index = checked(registry.capability_index(), 6, "CATALOG_ERROR")?;
    // Identity is a digest of canonical, sorted definitions rather than a local path.
    let definitions = registry
        .agents()
        .iter()
        .map(|a| a.to_json())
        .chain(registry.skills().iter().map(|s| s.to_json()))
        .collect::<Result<Vec<_>, _>>();
    let identity = gateway_application::resolution::ContentFingerprint::of_bytes(
        value(&checked(definitions, 6, "CATALOG_ERROR")?)?
            .to_string()
            .as_bytes(),
    );
    let snapshot = checked(
        PlanningCapabilitySnapshot::new(index, identity.as_str(), PlanningIrVersion::V1),
        5,
        "PLANNING_FAILED",
    )?;
    let app = DeclarativePlanningApplication::new();
    let comparison = ComparisonRules::default();
    let delta_rules = DeltaDerivationRules::default();
    let planner_rules = PlannerRules::default();
    // Hash the source ID so both Delta and derived Plan IDs stay within domain limits.
    let delta_id = checked(
        DeltaId::new(format!(
            "delta-{}",
            gateway_application::resolution::ContentFingerprint::of_bytes(
                desired.id().as_str().as_bytes()
            )
            .as_str()
        )),
        5,
        "PLANNING_FAILED",
    )?;
    let delta = checked(
        app.derive_delta(
            delta_id,
            &desired,
            assessment.document.observed_state(),
            Some(assessment.document.situation()),
            &comparison,
            &delta_rules,
        ),
        5,
        "PLANNING_FAILED",
    )?
    .into_delta();
    let requirements = checked(
        app.derive_capability_requirements(&desired, &delta, &snapshot, &capability_rules),
        5,
        "PLANNING_FAILED",
    )?;
    let result = checked(
        app.build_plan(&desired, &delta, &requirements, &planner_rules),
        5,
        "PLANNING_FAILED",
    )?;
    let Some(plan) = result.plan() else {
        let diagnostics = result
            .diagnostics()
            .iter()
            .map(|diagnostic| {
                json!({
                    "code":diagnostic.code().as_str(),
                    "delta_item":diagnostic.delta_item().map(|id| id.as_str()),
                    "blocking":diagnostic.is_blocking(),
                    "rationale":diagnostic.rationale(),
                })
            })
            .collect::<Vec<_>>();
        return Ok((
            json!({"schema_version":1,
            "error":{"code":"PLANNING_INCOMPLETE","message":"required outcomes could not form a valid Plan"},
            "assessment":assessment,"desired_state":desired,"delta":delta,"plan":null,
            "diagnostics":diagnostics,"capability_snapshot":snapshot.identity()}),
            5,
        ));
    };
    let explanation = checked(
        app.explain_plan(
            &desired,
            &delta,
            &result,
            &snapshot,
            PlanningRuleSnapshot::from_rules(
                &comparison,
                &delta_rules,
                &capability_rules,
                &planner_rules,
            ),
        ),
        5,
        "PLANNING_FAILED",
    )?
    .to_text();
    Ok((
        value(&PlanDocument {
            schema_version: 1,
            assessment,
            desired_state: desired,
            delta,
            plan: plan.clone(),
            explanation,
        })?,
        0,
    ))
}
pub(super) fn capture_resolution(
    options: &Options,
) -> Result<
    (
        ResolutionSnapshotInput,
        gateway_application::resolution_composition::CompositionRules,
        Value,
    ),
    CliError,
> {
    let document: PlanDocument = decode(options.input("plan")?)?;
    version(document.schema_version)?;
    version(document.assessment.schema_version)?;
    let rules: Rules = options.optional("rules")?;
    version(rules.schema_version)?;
    let registry = registry(options)?;
    let index = checked(registry.capability_index(), 6, "CATALOG_ERROR")?;
    let processes = checked(
        ProcessRegistry::load(
            Path::new(options.get("catalog").unwrap_or("catalog")).join("processes"),
        ),
        7,
        "PROCESS_CATALOG_ERROR",
    )?;
    let process: Option<ProcessInput> = options
        .get("process")
        .map(|s| read_json(s, false).and_then(decode))
        .transpose()?;
    let mut process_json = Value::Null;
    let (instance, expected_revision, situation_process) = if let Some(process) = process {
        version(process.schema_version)?;
        let definition = processes
            .get(
                process.instance.definition_id(),
                process.instance.definition_version(),
            )
            .ok_or_else(|| {
                CliError::new(
                    7,
                    "PROCESS_NOT_FOUND",
                    "instance definition is absent from the catalog",
                )
            })?;
        let reference = checked(
            DeclarativeSituationApplication::new().process_reference(
                ProcessSnapshotInput::new(definition, &process.instance)
                    .requiring_revision(process.expected_revision),
            ),
            7,
            "INVALID_PROCESS",
        )?;
        process_json = checked(
            serde_json::from_str(&checked(
                reference.inspection().to_json(),
                7,
                "INVALID_PROCESS",
            )?),
            7,
            "INVALID_PROCESS",
        )?;
        (
            Some(process.instance),
            Some(process.expected_revision),
            Some(reference),
        )
    } else {
        (None, None, None)
    };
    let assessment = document.assessment;
    let input = ResolutionSnapshotInput {
        version: SchemaVersion::V1,
        scope: assessment.scope.clone(),
        plan_scope: assessment.scope.clone(),
        situation_scope: assessment.scope,
        plan: document.plan,
        desired: document.desired_state,
        delta: document.delta,
        situation: assessment.document,
        operating_mode: assessment.operating_mode,
        execution_profile: assessment.execution_profile,
        registry,
        index,
        processes,
        instance,
        expected_revision,
        situation_process,
        admission: None,
        rule_version: SchemaVersion::V1,
        alternatives: vec![],
    };
    Ok((input, rules.resolution.build()?, process_json))
}
fn resolve(options: &Options) -> Result<(ResolvedPlan, Value), CliError> {
    let (input, rules, process) = capture_resolution(options)?;
    let resolved = checked(
        DeclarativeResolutionApplication.resolve_plan(&input, &rules),
        6,
        "RESOLUTION_FAILED",
    )?;
    Ok((resolved, process))
}
fn basis_check(claimed: &Value, actual: &Value, exit: i32) -> Result<(), CliError> {
    if claimed != actual {
        return Err(CliError::new(
            exit,
            "STALE_BASIS",
            "input basis differs from the current resolution snapshot",
        ));
    }
    Ok(())
}
fn policy(
    options: &Options,
    resolved: &ResolvedPlan,
    basis: &Value,
) -> Result<(PolicyAuthority, PolicyContext, Value, i32), CliError> {
    let input: PolicyInput = decode(read_json(options.required("policy")?, true)?)?;
    version(input.schema_version)?;
    basis_check(&input.basis, basis, 8)?;
    let authority = PolicyAuthority {
        policies: input
            .policies
            .into_iter()
            .map(PolicyDefinitionInput::build)
            .collect::<Result<_, _>>()?,
        capabilities: resolved
            .snapshot
            .input()
            .index
            .entries()
            .map(|e| (e.id().clone(), e.capability().clone()))
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
    let report = checked(
        PolicyApplication.evaluate(resolved, &authority, &context),
        8,
        "POLICY_FAILED",
    )?;
    let exit = if report.decision() == PolicyDecision::Allow {
        0
    } else {
        8
    };
    let json = checked(
        serde_json::from_str(&checked(report.to_json(), 8, "POLICY_FAILED")?),
        8,
        "POLICY_FAILED",
    )?;
    Ok((authority, context, json, exit))
}
fn downstream(options: &Options) -> Result<(Value, i32), CliError> {
    let (resolved, process) = resolve(options)?;
    let app = DeclarativeResolutionApplication;
    let artifact: Value = checked(
        serde_json::from_str(&checked(
            app.serialize_resolution(&resolved, Default::default()),
            6,
            "RESOLUTION_FAILED",
        )?),
        6,
        "RESOLUTION_FAILED",
    )?;
    let trace = checked(
        app.explain_resolution(
            &resolved,
            gateway_application::resolution_explain::TraceLimits {
                max_nodes: 100000,
                max_optional_details: 10000,
            },
        ),
        6,
        "RESOLUTION_FAILED",
    )?;
    let mut exit = if matches!(
        resolved.report.outcome,
        ResolutionOutcome::Resolved | ResolutionOutcome::NoOp
    ) {
        0
    } else {
        6
    };
    if exit == 0
        && resolved
            .report
            .alternatives
            .iter()
            .flatten()
            .any(|alternative| {
                matches!(
                    alternative.applicability.readiness,
                    gateway_application::resolution::LifecycleReadiness::Blocked
                        | gateway_application::resolution::LifecycleReadiness::Deferred
                        | gateway_application::resolution::LifecycleReadiness::Unknown
                )
            })
    {
        exit = 7;
    }
    let evaluated = options
        .get("policy")
        .map(|_| policy(options, &resolved, &artifact["basis"]))
        .transpose()?;
    if let Some((_, _, _, policy_exit)) = &evaluated {
        if *policy_exit != 0 {
            exit = *policy_exit;
        }
    }
    if options.command == "compile" {
        let (authority, context, report, policy_exit) =
            evaluated.expect("compile parser requires policy");
        if policy_exit != 0 {
            return Ok((
                json!({"schema_version":1,"error":{"code":"POLICY_NOT_ALLOWED","message":"compilation requires ALLOW"},"policy":report}),
                8,
            ));
        }
        let command = map_compile(
            read_json(options.required("projection")?, true)?,
            resolved,
            authority,
            context,
            &artifact["basis"],
        )?;
        let compiled = checked(
            ContextApplication.compile_step(CompileStepInput {
                resolved: &command.resolved,
                authority: &command.authority,
                policy_context: &command.policy_context,
                catalog: &command.catalog,
                projection: &command.projection,
                candidates: &command.candidates,
                selected: &command.selected,
            }),
            9,
            "COMPILATION_FAILED",
        )?;
        return Ok((
            checked(
                serde_json::from_str(&checked(compiled.to_json(), 9, "COMPILATION_FAILED")?),
                9,
                "COMPILATION_FAILED",
            )?,
            0,
        ));
    }
    Ok((
        json!({"schema_version":1,"resolution":artifact,"explanation":checked(serde_json::from_str::<Value>(&trace.to_json()),6,"RESOLUTION_FAILED")?,
        "assessment":resolved.snapshot.input().situation,"delta":resolved.snapshot.input().delta,"plan":resolved.snapshot.input().plan,
        "process":process,"policy":evaluated.map(|(_,_,report,_)| report)}),
        exit,
    ))
}

pub(super) fn map_compile(
    document: Value,
    resolved: ResolvedPlan,
    authority: PolicyAuthority,
    context: PolicyContext,
    basis: &Value,
) -> Result<gateway_application::codex::CompileCommand, CliError> {
    let input: ProjectionInput = decode(document)?;
    version(input.schema_version)?;
    basis_check(&input.basis, basis, 9)?;
    let catalog = checked(
        DefinitionCatalog::new(
            resolved
                .snapshot
                .input()
                .registry
                .agents()
                .iter()
                .map(|a| a.to_domain())
                .collect(),
            resolved
                .snapshot
                .input()
                .registry
                .skills()
                .iter()
                .map(|s| s.to_domain())
                .collect(),
            input
                .workflows
                .into_iter()
                .map(WorkflowInput::build)
                .collect::<Result<_, _>>()?,
            authority.policies.clone(),
        ),
        9,
        "INVALID_PROJECTION_CATALOG",
    )?;
    let projection = ContextProjection {
        mapping: WorkflowProjectionMapping {
            basis: resolved.report.basis.clone(),
            step: input.step,
            task: input.task.id().clone(),
            process: input.process,
            workflow: input.workflow,
            decision_reference: input.decision_reference,
        },
        id: input.id,
        task: input.task,
        state: input.state,
        state_basis: resolved.report.basis.clone(),
        state_decision: input.state_decision,
        target_runtime: input.target_runtime,
        knowledge_queries: input.knowledge_queries,
    };
    let candidates = input
        .fragments
        .into_iter()
        .map(|fragment| fragment.build(resolved.snapshot.input().situation.records()))
        .collect::<Result<Vec<_>, _>>()?;
    Ok(gateway_application::codex::CompileCommand {
        resolved,
        authority,
        policy_context: context,
        catalog,
        projection,
        candidates,
        selected: input.selected,
        disclosure: gateway_context::ContextDisclosurePolicy {
            maximum_sensitivity: SensitivityClass::Normal,
            include_caller_input: false,
            include_external_content: false,
        },
    })
}
