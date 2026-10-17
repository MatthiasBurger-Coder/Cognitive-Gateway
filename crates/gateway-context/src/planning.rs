//! Actual CG-10 producer handoff. Pure validation/assembly, never invocation admission.
use crate::{CompiledContext, ContextCompiler, FragmentKind};
use gateway_domain::semantic_output::{OutputContract, VerificationContract, validate_contracts};
use gateway_domain::*;
use serde_json::{Value, json};
use std::collections::{BTreeMap, BTreeSet};

/// Lossless planning supplement consumed by the #192 boundary. Output and
/// verification remain in the complete task; stable instructions stay in CG-10.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PlannedContextHandoff {
    task: SemanticTaskIR,
    requirements: ContextRequirementsIR,
    plan: ContextPlanIR,
    context: CompiledContext,
    context_binding: ResolvedReference<ReferenceId>,
    projection_binding: ResolvedReference<ReferenceId>,
    satisfaction: BTreeMap<ReferenceId, ContextSourceMetadata>,
    contracts: ContextHandoffContracts,
    examples: Option<ExampleSet>,
}
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ContextHandoffContracts {
    pub output: OutputContract,
    pub verification: VerificationContract,
    pub verification_revision: ReferenceId,
}
/// Canonical set plus the upstream expected pin. The host authenticates this
/// pin and current quarantine before the pure CG-10 boundary is entered.
pub struct ContextHandoffWithExamples {
    pub contracts: ContextHandoffContracts,
    pub examples: ExampleSet,
    pub expected_examples: ResolvedReference<ReferenceId>,
}
impl PlannedContextHandoff {
    pub fn examples(&self) -> Option<&ExampleSet> {
        self.examples.as_ref()
    }

    /// Disclosure-limited examples for audit only. Redacted output is never
    /// invocation input; the immutable full set and its pin remain unchanged.
    pub fn example_audit_value(&self, policy: crate::ContextDisclosurePolicy) -> Option<Value> {
        self.examples.as_ref().map(|examples| {
            if !policy.include_external_content || examples.selected().iter().any(|c|
                c.quality().sensitivity() > policy.maximum_sensitivity) {
                json!({"binding":examples.binding(),"representation":"redacted",
                    "selected":examples.selected().iter().map(|c|
                        json!({"id":c.pin.id,"quality":c.quality(),"content":"[REDACTED]"})).collect::<Vec<_>>()})
            } else { examples.to_value() }
        })
    }
    pub fn task(&self) -> &SemanticTaskIR {
        &self.task
    }
    pub fn requirements(&self) -> &ContextRequirementsIR {
        &self.requirements
    }
    pub fn plan(&self) -> &ContextPlanIR {
        &self.plan
    }
    pub fn context(&self) -> &CompiledContext {
        &self.context
    }
    pub fn to_value(&self) -> Value {
        let compiled: Value = serde_json::from_str(&self.context.to_json().expect("compiled JSON"))
            .expect("compiled JSON");
        let fragments = compiled["dynamic"].as_array().expect("compiled fragments");
        json!({"schema_version": SchemaVersion::V1,
            "bindings":{"task":self.requirements.basis().task,
                "requirements":self.requirements.binding(),"plan":self.plan.binding(),
                "context":self.context_binding,"execution_projection":self.projection_binding,
                "examples":self.examples.as_ref().map(ExampleSet::binding),
                "output":binding("output-definition", &self.contracts.output.schema.revision, &self.context_binding.scope,
                    &serde_json::to_value(&self.contracts.output).expect("output JSON")),
                "verification":binding("verification-definition", &self.contracts.verification_revision, &self.context_binding.scope,
                    &serde_json::to_value(&self.contracts.verification).expect("verification JSON"))},
            "task":self.task,"instructions":compiled["stable"],
            "context":{"scope":compiled["scope"],"step":compiled["step"],"execution_context":compiled["execution_context"],
                "fragments":fragments.iter().filter(|f| f["kind"] != "evidence").collect::<Vec<_>>()},
            "evidence":fragments.iter().filter(|f| f["kind"] == "evidence").collect::<Vec<_>>(),
            "examples":self.examples.as_ref().map(ExampleSet::to_value),
            "planning":{"requirements":self.requirements.to_value(),"plan":self.plan.to_value(),
                "satisfaction":self.satisfaction.iter().map(|(r,s)| json!({"requirement":r,
                    "source":s.to_value(),"link":s.present,"tokens":s.tokens})).collect::<Vec<_>>()},
            "output":self.contracts.output,"verification":self.contracts.verification})
    }
}
impl ContextCompiler {
    /// Authenticate the expected snapshot pin upstream. This checks exact bytes,
    /// scope, step, constraints and satisfaction; it neither fetches nor authorizes.
    pub fn handoff_planned_context(
        task: SemanticTaskIR,
        requirements: ContextRequirementsIR,
        plan: ContextPlanIR,
        context: CompiledContext,
        expected_context: ResolvedReference<ReferenceId>,
        acquired: &[ContextSourceMetadata],
        contracts: ContextHandoffContracts,
    ) -> Result<PlannedContextHandoff, ContextPlanningError> {
        Self::assemble_planned_context(
            task,
            requirements,
            plan,
            context,
            expected_context,
            acquired,
            (contracts, None),
        )
    }
    pub fn handoff_planned_context_with_examples(
        task: SemanticTaskIR,
        requirements: ContextRequirementsIR,
        plan: ContextPlanIR,
        context: CompiledContext,
        expected_context: ResolvedReference<ReferenceId>,
        acquired: &[ContextSourceMetadata],
        options: ContextHandoffWithExamples,
    ) -> Result<PlannedContextHandoff, ContextPlanningError> {
        if options.examples.binding() != options.expected_examples {
            return Err(ContextPlanningError::BindingMismatch);
        }
        Self::assemble_planned_context(
            task,
            requirements,
            plan,
            context,
            expected_context,
            acquired,
            (options.contracts, Some(options.examples)),
        )
    }
    fn assemble_planned_context(
        task: SemanticTaskIR,
        requirements: ContextRequirementsIR,
        plan: ContextPlanIR,
        context: CompiledContext,
        expected_context: ResolvedReference<ReferenceId>,
        acquired: &[ContextSourceMetadata],
        options: (ContextHandoffContracts, Option<ExampleSet>),
    ) -> Result<PlannedContextHandoff, ContextPlanningError> {
        let (contracts, examples) = options;
        use ContextPlanningError as E;
        if let Some(examples) = &examples {
            if examples.requirement().requirements() != &requirements.binding() {
                return Err(E::BindingMismatch);
            }
            // Revalidate immutable canonical producer output and its exact requirement.
            if !requirements
                .requirements()
                .contains(examples.requirement().context())
            {
                return Err(E::InvalidSatisfaction);
            }
        }
        validate_contracts(&task, &contracts.output, &contracts.verification)
            .map_err(|_| E::BindingMismatch)?;
        let value: Value = serde_json::from_str(&context.to_json().expect("compiled JSON"))
            .expect("compiled JSON");
        let actual = binding(
            "compiled-context",
            &expected_context.revision,
            &expected_context.scope,
            &value,
        );
        let basis = requirements.basis();
        if actual != expected_context
            || basis.task.digest != task.content_digest().map_err(|_| E::BindingMismatch)?
            || basis.task.id != task.data().id
            || basis.task.contract_version != task.data().schema_version
            || plan.requirements_binding() != &requirements.binding()
            || context.execution_context().task().id() != &task.data().id
        {
            return Err(E::BindingMismatch);
        }
        if value["scope"] != basis.task.scope.as_str()
            || expected_context.scope != basis.task.scope
            || task.data().target.reference.scope != basis.task.scope
        {
            return Err(E::ScopeMismatch);
        }
        let example_source = examples.as_ref().and_then(ExampleSet::source);
        let mut acquisition = BTreeMap::new();
        for s in acquired {
            if acquisition.insert(&s.pin.id, s).is_some() {
                return Err(E::InvalidSatisfaction);
            }
            if !plan
                .selected()
                .values()
                .any(|v| v.pin == s.pin && v.present.is_none())
            {
                return Err(E::InvalidSatisfaction);
            }
        }
        if let Some(s) = &example_source {
            if plan
                .selected()
                .values()
                .any(|p| p.pin == s.pin && p.present.is_none())
                && acquisition.insert(&s.pin.id, s).is_some()
            {
                return Err(E::InvalidSatisfaction);
            }
        }
        let mut satisfaction = BTreeMap::new();
        let mut charged = BTreeSet::new();
        let mut used = 0u64;
        for r in requirements.requirements() {
            if !r.necessity.active() {
                continue;
            }
            let source = plan.selected().get(&r.id).and_then(|s| {
                if s.present.is_some() {
                    Some(s)
                } else {
                    acquisition.get(&s.pin.id).copied()
                }
            });
            let Some(source) = source else {
                if r.necessity.required() {
                    return Err(E::MissingMandatory);
                }
                continue;
            };
            let planned = &plan.selected()[&r.id];
            if source.pin != planned.pin
                || source.class != planned.class
                || source.tokens > planned.tokens
                || planned
                    .payload_digest
                    .as_ref()
                    .is_some_and(|d| source.payload_digest.as_ref() != Some(d))
                || context_source_rejection(r, source, &basis.task.scope).is_some()
            {
                return Err(E::InvalidSatisfaction);
            }
            if let Some(ContextSatisfaction::Examples(_)) = &source.present {
                if r.class != ContextClass::Examples || example_source.as_ref() != Some(source) {
                    return Err(E::InvalidSatisfaction);
                }
            } else {
                validate_link(r, source, &task, &context, &value, &contracts)?;
            }
            if charged.insert(source.pin.id.clone()) {
                used = used.checked_add(source.tokens).ok_or(E::InfeasibleBudget)?;
                if used > requirements.total_tokens().0 {
                    return Err(E::InfeasibleBudget);
                }
            }
            satisfaction.insert(r.id.clone(), source.clone());
        }
        if example_source
            .as_ref()
            .is_some_and(|s| !satisfaction.values().any(|v| v == s))
        {
            return Err(E::InvalidSatisfaction);
        }
        // A compiled payload cannot smuggle context outside the canonical plan.
        let linked: BTreeSet<_> = satisfaction
            .values()
            .filter_map(|s| match &s.present {
                Some(ContextSatisfaction::Fragment(id)) => Some(id),
                _ => None,
            })
            .collect();
        if context.fragments().iter().any(|f| !linked.contains(f.id())) {
            return Err(E::InvalidSatisfaction);
        }
        let projection_binding = binding(
            "execution-projection",
            &expected_context.revision,
            &expected_context.scope,
            &value["execution_context"],
        );
        Ok(PlannedContextHandoff {
            task,
            requirements,
            plan,
            context,
            context_binding: expected_context,
            projection_binding,
            satisfaction,
            contracts,
            examples,
        })
    }
}
fn validate_link(
    r: &ContextRequirement,
    s: &ContextSourceMetadata,
    task: &SemanticTaskIR,
    context: &CompiledContext,
    compiled: &Value,
    contracts: &ContextHandoffContracts,
) -> Result<(), ContextPlanningError> {
    use ContextClass as C;
    let valid = match &s.present {
        Some(ContextSatisfaction::Task) => {
            r.class == C::TaskInput
                && s.payload_digest.as_ref()
                    == Some(&task.content_digest().expect("validated task"))
        }
        Some(ContextSatisfaction::OutputContract) => {
            r.class == C::OutputVerification
                && s.payload_digest.as_ref()
                    == Some(&context_content_digest(
                        serde_json::to_string(&json!({
                            "output":contracts.output,"verification":contracts.verification
                        }))
                        .expect("output JSON")
                        .as_bytes(),
                    ))
        }
        Some(ContextSatisfaction::Catalog) => {
            r.class == C::Authority
                && s.class == ContextSourceClass::CatalogAuthority
                && s.payload_digest.as_ref()
                    == Some(&context_content_digest(
                        serde_json::to_string(&compiled["stable"])
                            .expect("catalog JSON")
                            .as_bytes(),
                    ))
        }
        Some(ContextSatisfaction::Fragment(id)) => context.fragments().iter().any(|f| {
            let kind_ok = match r.class {
                C::Authority | C::TaskInput | C::OutputVerification | C::Examples => false,
                C::Evidence => f.kind() == FragmentKind::Evidence,
                C::Memory => f.kind() == FragmentKind::Memory,
                C::RuntimeState | C::SystemState => {
                    matches!(f.kind(), FragmentKind::Knowledge | FragmentKind::Evidence)
                }
                C::DomainKnowledge | C::StructuredData => f.kind() == FragmentKind::Knowledge,
            };
            kind_ok
                && f.id() == id
                && f.metadata().quality == s.quality
                && f.metadata().provenance.source() == s.pin.id.as_str()
                && f.metadata().provenance.revision() == Some(s.pin.revision.as_str())
                && Some(&context_content_digest(f.content().as_bytes()))
                    == s.payload_digest.as_ref()
                && s.evidence.iter().all(|e| {
                    f.metadata()
                        .evidence
                        .iter()
                        .any(|v| v.as_str() == e.as_str())
                })
                && s.provenances.iter().all(|p| {
                    f.metadata()
                        .evidence
                        .iter()
                        .any(|v| v.as_str() == p.as_str())
                })
        }),
        Some(ContextSatisfaction::Examples(_)) | None => false,
    };
    if valid {
        Ok(())
    } else {
        Err(ContextPlanningError::InvalidSatisfaction)
    }
}
