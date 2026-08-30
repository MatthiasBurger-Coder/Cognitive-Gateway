//! CG-07.06 deterministic graph semantics for declarative Plans.
//!
//! A Plan graph describes prerequisite order only.  It does not infer a
//! process lifecycle, authorize a mutation or select an executor.

use std::collections::{BTreeMap, BTreeSet};

use crate::{PlanStep, PlanStepId, ValidationError};

type GraphIndexes = (
    BTreeMap<PlanStepId, usize>,
    BTreeMap<PlanStepId, BTreeSet<PlanStepId>>,
);

/// Validates that all PlanStep dependency edges form a finite DAG.
pub(crate) fn validate_steps(steps: &[PlanStep]) -> Result<(), ValidationError> {
    let _ = topological_order(steps)?;
    Ok(())
}

/// Returns PlanStep identities in deterministic dependency-first order.
pub(crate) fn topological_order(steps: &[PlanStep]) -> Result<Vec<PlanStepId>, ValidationError> {
    let (mut indegrees, successors) = graph_indexes(steps)?;
    let mut ready = indegrees
        .iter()
        .filter(|(_, degree)| **degree == 0)
        .map(|(id, _)| id.clone())
        .collect::<BTreeSet<_>>();
    let mut order = Vec::with_capacity(steps.len());

    while let Some(id) = ready.pop_first() {
        order.push(id.clone());
        if let Some(dependent_steps) = successors.get(&id) {
            for dependent in dependent_steps {
                let degree = indegrees
                    .get_mut(dependent)
                    .expect("successor must have an indexed step");
                *degree -= 1;
                if *degree == 0 {
                    ready.insert(dependent.clone());
                }
            }
        }
    }

    if order.len() != steps.len() {
        return Err(ValidationError::CircularRelationship {
            field: "plan_step.dependencies",
        });
    }
    Ok(order)
}

/// Returns deterministic antichain layers that can run independently once
/// all preceding layers have completed.
pub(crate) fn parallel_layers(steps: &[PlanStep]) -> Result<Vec<Vec<PlanStepId>>, ValidationError> {
    let (mut indegrees, successors) = graph_indexes(steps)?;
    let mut ready = indegrees
        .iter()
        .filter(|(_, degree)| **degree == 0)
        .map(|(id, _)| id.clone())
        .collect::<BTreeSet<_>>();
    let mut layers = Vec::new();
    let mut visited = 0;

    while !ready.is_empty() {
        let layer = ready.iter().cloned().collect::<Vec<_>>();
        ready.clear();
        visited += layer.len();
        for id in &layer {
            if let Some(dependent_steps) = successors.get(id) {
                for dependent in dependent_steps {
                    let degree = indegrees
                        .get_mut(dependent)
                        .expect("successor must have an indexed step");
                    *degree -= 1;
                    if *degree == 0 {
                        ready.insert(dependent.clone());
                    }
                }
            }
        }
        layers.push(layer);
    }

    if visited != steps.len() {
        return Err(ValidationError::CircularRelationship {
            field: "plan_step.dependencies",
        });
    }
    Ok(layers)
}

fn graph_indexes(steps: &[PlanStep]) -> Result<GraphIndexes, ValidationError> {
    let mut indegrees = BTreeMap::new();
    for step in steps {
        if indegrees.insert(step.id().clone(), 0).is_some() {
            return Err(ValidationError::DuplicateDeclarativeIdentity {
                kind: "plan_step",
                id: step.id().to_string(),
            });
        }
    }

    let mut successors = BTreeMap::<PlanStepId, BTreeSet<PlanStepId>>::new();
    for step in steps {
        for dependency in step.dependencies() {
            if !indegrees.contains_key(dependency) {
                return Err(ValidationError::MissingDeclarativeIdentity {
                    kind: "plan_step",
                    id: dependency.to_string(),
                });
            }
            *indegrees
                .get_mut(step.id())
                .expect("step was indexed before edge validation") += 1;
            successors
                .entry(dependency.clone())
                .or_default()
                .insert(step.id().clone());
        }
    }
    Ok((indegrees, successors))
}

#[cfg(test)]
mod tests {
    use crate::{
        ConditionId, PlanCondition, PlanStep, PlanStepId, PlanStepKind, RequiredOutcome,
        RequiredOutcomeKind,
    };

    use super::*;

    fn step(id: &str, dependencies: &[&str]) -> PlanStep {
        PlanStep::new(
            PlanStepId::new(id).unwrap(),
            PlanStepKind::Change,
            RequiredOutcome::new(RequiredOutcomeKind::DomainChange, "change state").unwrap(),
            PlanCondition::desired_condition(ConditionId::new("condition-1").unwrap()),
            "graph test step",
        )
        .unwrap()
        .with_dependencies(
            dependencies
                .iter()
                .map(|id| PlanStepId::new(*id).unwrap())
                .collect(),
        )
        .unwrap()
    }

    #[test]
    fn orders_chains_and_groups_independent_layers() {
        let steps = vec![
            step("verify", &["change-a", "change-b"]),
            step("change-b", &[]),
            step("change-a", &[]),
            step("independent", &[]),
        ];
        assert_eq!(
            topological_order(&steps).unwrap(),
            [
                PlanStepId::new("change-a").unwrap(),
                PlanStepId::new("change-b").unwrap(),
                PlanStepId::new("independent").unwrap(),
                PlanStepId::new("verify").unwrap(),
            ]
        );
        assert_eq!(
            parallel_layers(&steps).unwrap(),
            [
                vec![
                    PlanStepId::new("change-a").unwrap(),
                    PlanStepId::new("change-b").unwrap(),
                    PlanStepId::new("independent").unwrap(),
                ],
                vec![PlanStepId::new("verify").unwrap()],
            ]
        );
    }

    #[test]
    fn rejects_cycles_dangling_edges_and_duplicate_nodes() {
        let cycle_a = step("a", &["b"]);
        let cycle_b = step("b", &["a"]);
        assert!(matches!(
            topological_order(&[cycle_a, cycle_b]),
            Err(ValidationError::CircularRelationship { .. })
        ));
        assert!(matches!(
            topological_order(&[step("a", &["missing"])]),
            Err(ValidationError::MissingDeclarativeIdentity {
                kind: "plan_step",
                ..
            })
        ));
        let duplicate = step("a", &[]);
        assert!(matches!(
            topological_order(&[duplicate.clone(), duplicate]),
            Err(ValidationError::DuplicateDeclarativeIdentity {
                kind: "plan_step",
                ..
            })
        ));
    }

    #[test]
    fn validates_empty_graph() {
        assert!(validate_steps(&[]).is_ok());
        assert!(topological_order(&[]).unwrap().is_empty());
        assert!(parallel_layers(&[]).unwrap().is_empty());
    }
}
