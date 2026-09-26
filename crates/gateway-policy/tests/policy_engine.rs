use gateway_domain::*;
use gateway_policy::*;
use std::collections::{BTreeMap, BTreeSet};

fn cap() -> CapabilityId {
    CapabilityId::new("repository.read").unwrap()
}
fn authority(class: CapabilityClass) -> PolicyAuthority {
    PolicyAuthority {
        policies: vec![
            PolicyDefinition::new(PolicyId::new("baseline").unwrap(), "baseline", [cap()]).unwrap(),
        ],
        capabilities: BTreeMap::from([(cap(), CapabilityDefinition::new(cap(), class))]),
        ..Default::default()
    }
}
fn facts() -> StepFacts {
    StepFacts {
        authorizations: BTreeMap::from([(cap(), Approval::Granted)]),
        ..Default::default()
    }
}
fn evaluate(a: &PolicyAuthority, f: &StepFacts) -> StepPolicyReport {
    PolicyEngine::evaluate(
        a,
        &StepPolicyInput {
            step: &PlanStepId::new("step").unwrap(),
            capabilities: &a.capabilities,
            operating_mode: OperatingMode::Development,
            execution_profile: ExecutionProfile::FastPath,
            process: ProcessReadiness::NotApplicable,
            resolved: true,
            has_prerequisites: false,
            constraints: &BTreeSet::new(),
            preconditions: &BTreeSet::new(),
            facts: f,
        },
    )
}
fn reason(report: &StepPolicyReport, reason: PolicyReason) -> bool {
    report.findings.iter().any(|f| f.reason == reason)
}
#[test]
fn inspect_and_mutate_are_separate_and_machine_readable() {
    let a = authority(CapabilityClass::Inspect);
    let f = facts();
    let report = evaluate(&a, &f);
    assert_eq!(report, evaluate(&a, &f));
    assert_eq!(report.decision, PolicyDecision::Allow);
    assert_eq!(report.capability_classes[&cap()], "INSPECT");
    let json = serde_json::to_value(&report).unwrap();
    assert_eq!(json["decision"], "ALLOW");
    assert_eq!(json["step"], "step");
    assert_eq!(json["findings"][0]["reason"], "ALLOWED");
    let a = authority(CapabilityClass::Mutate);
    assert_eq!(evaluate(&a, &f).decision, PolicyDecision::RequireConsent);
    let mut f = f;
    f.consents.insert(cap(), Approval::Denied);
    assert!(reason(&evaluate(&a, &f), PolicyReason::ConsentDenied));
    f.consents.insert(cap(), Approval::Granted);
    assert_eq!(evaluate(&a, &f).decision, PolicyDecision::Allow);
    assert_eq!(evaluate(&a, &f).capability_classes[&cap()], "MUTATE");
}
#[test]
fn no_authorization_is_inferred_from_an_allowlist() {
    let a = authority(CapabilityClass::Inspect);
    let mut f = StepFacts::default();
    assert!(reason(
        &evaluate(&a, &f),
        PolicyReason::AuthorizationMissing
    ));
    f.authorizations.insert(cap(), Approval::Denied);
    assert!(reason(&evaluate(&a, &f), PolicyReason::AuthorizationDenied));
}
#[test]
fn deny_overrides_allow_consent_evidence_and_policy_order() {
    let mut a = authority(CapabilityClass::Mutate);
    a.policies.push(
        PolicyDefinition::with_denied_capabilities(
            PolicyId::new("deny").unwrap(),
            "deny",
            [],
            [cap()],
        )
        .unwrap(),
    );
    let mut f = facts();
    f.consents.insert(cap(), Approval::Granted);
    let report = evaluate(&a, &f);
    assert_eq!(report.decision, PolicyDecision::Deny);
    assert!(reason(&report, PolicyReason::ExplicitDeny));
    a.policies.reverse();
    assert_eq!(report, evaluate(&a, &f));
    a.policies.clear();
    assert!(reason(&evaluate(&a, &f), PolicyReason::NotAllowlisted));
    a.policies
        .push(PolicyDefinition::new(PolicyId::new("empty").unwrap(), "empty", []).unwrap());
    assert!(reason(&evaluate(&a, &f), PolicyReason::NotAllowlisted));
}
#[test]
fn evidence_and_constraints_are_exact_and_cannot_grant_authority() {
    let mut a = authority(CapabilityClass::Inspect);
    a.capabilities.insert(
        cap(),
        CapabilityDefinition::new(cap(), CapabilityClass::Inspect)
            .with_preconditions(["repository.available"])
            .unwrap()
            .with_constraints(["read-only"])
            .unwrap(),
    );
    a.required_evidence
        .insert(cap(), BTreeSet::from(["review.passed".into()]));
    let mut f = facts();
    let report = evaluate(&a, &f);
    assert_eq!(report.decision, PolicyDecision::RequireEvidence);
    assert!(reason(&report, PolicyReason::EvidenceMissing));
    assert!(reason(&report, PolicyReason::ConstraintUnsatisfied));
    f.evidence
        .extend(["repository.available".into(), "review.passed".into()]);
    f.satisfied_constraints.insert("read-only".into());
    assert_eq!(evaluate(&a, &f).decision, PolicyDecision::Allow);
    f.authorizations.clear();
    assert_eq!(evaluate(&a, &f).decision, PolicyDecision::RequireConsent);
}
#[test]
fn canonical_contracts_reject_reclassification_and_unknown_capabilities() {
    let a = authority(CapabilityClass::Mutate);
    let proposed = authority(CapabilityClass::Inspect).capabilities;
    let f = facts();
    let mut input = StepPolicyInput {
        step: &PlanStepId::new("step").unwrap(),
        capabilities: &proposed,
        operating_mode: OperatingMode::Development,
        execution_profile: ExecutionProfile::FullPath,
        process: ProcessReadiness::Eligible,
        resolved: true,
        has_prerequisites: false,
        constraints: &BTreeSet::new(),
        preconditions: &BTreeSet::new(),
        facts: &f,
    };
    let report = PolicyEngine::evaluate(&a, &input);
    assert!(reason(&report, PolicyReason::ContractMismatch));
    assert_eq!(report.capability_classes[&cap()], "MUTATE");
    assert!(reason(
        &PolicyEngine::evaluate(&PolicyAuthority::default(), &input),
        PolicyReason::UnknownCapability
    ));
    input.resolved = false;
    input.process = ProcessReadiness::Blocked;
    input.has_prerequisites = true;
    let report = PolicyEngine::evaluate(&a, &input);
    assert!(reason(&report, PolicyReason::Unresolved));
    assert!(reason(&report, PolicyReason::ProcessBlocked));
    assert!(reason(&report, PolicyReason::PrerequisiteMissing));
    input.process = ProcessReadiness::Unknown;
    assert!(reason(
        &PolicyEngine::evaluate(&a, &input),
        PolicyReason::ProcessUnknown
    ));
}
#[test]
fn governance_feature_freeze_and_release_depth_are_independent() {
    let mut a = authority(CapabilityClass::Inspect);
    a.constraints.push(Constraint::new(
        ConstraintId::new("freeze").unwrap(),
        ConstraintKind::FeatureFreeze,
    ));
    a.constraints.push(Constraint::new(
        ConstraintId::new("depth").unwrap(),
        ConstraintKind::RequireFullPathForReleaseQualification,
    ));
    a.constraints.push(Constraint::new(
        ConstraintId::new("consent").unwrap(),
        ConstraintKind::LiveMutationRequiresConsent,
    ));
    let mut f = facts();
    assert!(reason(&evaluate(&a, &f), PolicyReason::WorkClassUnknown));
    f.work_class = Some(WorkClass::Feature);
    assert!(reason(&evaluate(&a, &f), PolicyReason::FeatureFrozen));
    f.work_class = Some(WorkClass::Maintenance);
    assert_eq!(evaluate(&a, &f).decision, PolicyDecision::Allow);
    for mode in [
        OperatingMode::Development,
        OperatingMode::Hardening,
        OperatingMode::ReleaseQualification,
    ] {
        for profile in [
            ExecutionProfile::FastPath,
            ExecutionProfile::NormalPath,
            ExecutionProfile::FullPath,
        ] {
            let report = PolicyEngine::evaluate(
                &a,
                &StepPolicyInput {
                    step: &PlanStepId::new("step").unwrap(),
                    capabilities: &a.capabilities,
                    operating_mode: mode,
                    execution_profile: profile,
                    process: ProcessReadiness::Eligible,
                    resolved: true,
                    has_prerequisites: false,
                    constraints: &BTreeSet::new(),
                    preconditions: &BTreeSet::new(),
                    facts: &f,
                },
            );
            assert_eq!(
                report.decision == PolicyDecision::Deny,
                mode == OperatingMode::ReleaseQualification
                    && profile != ExecutionProfile::FullPath
            );
        }
    }
}

#[test]
fn trusted_attestations_cannot_override_read_only_semantics() {
    let mut a = authority(CapabilityClass::Mutate);
    a.capabilities.insert(
        cap(),
        CapabilityDefinition::new(cap(), CapabilityClass::Mutate)
            .with_constraints(["read-only"])
            .unwrap(),
    );
    let mut f = facts();
    f.consents.insert(cap(), Approval::Granted);
    f.satisfied_constraints.insert("read-only".into());
    let report = evaluate(&a, &f);
    assert_eq!(report.decision, PolicyDecision::Deny);
    assert!(reason(&report, PolicyReason::ConstraintViolation));
}
