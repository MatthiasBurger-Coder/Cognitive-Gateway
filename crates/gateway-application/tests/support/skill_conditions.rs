use super::*;
#[test]
fn comparison_projection_keeps_conflict_and_unsupported_distinct() {
    assert_eq!(
        condition_status(ComparisonOutcome::Satisfied),
        ConditionStatus::Satisfied
    );
    assert_eq!(
        condition_status(ComparisonOutcome::Unsatisfied),
        ConditionStatus::Unsatisfied
    );
    assert_eq!(
        condition_status(ComparisonOutcome::Conflicted),
        ConditionStatus::Conflicted
    );
    assert_eq!(
        condition_status(ComparisonOutcome::Incomparable),
        ConditionStatus::Unsupported
    );
    for outcome in [
        ComparisonOutcome::Unknown,
        ComparisonOutcome::InsufficientEvidence,
        ComparisonOutcome::UnresolvedInput,
    ] {
        assert_eq!(condition_status(outcome), ConditionStatus::Unknown);
    }
}
