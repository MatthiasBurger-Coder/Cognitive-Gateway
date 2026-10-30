// Reuse public contract cases in the owning library and external consumers.
// Broad coverage must include the library's own non-generic contract methods.
#[path = "../tests/procedure_promotion.rs"]
mod procedure_promotion;
#[path = "../tests/semantic_epistemic.rs"]
mod semantic_epistemic;
#[path = "../tests/semantic_resolution.rs"]
mod semantic_resolution;

use crate::retrieval_plane::{RetrievalError, RetrievalVersion, SufficiencyFinding};

#[test]
fn retrieval_version_admission_never_falls_back_from_an_unknown_version() {
    for (wire, expected) in [(1, RetrievalVersion::V1), (2, RetrievalVersion::V2)] {
        assert_eq!(RetrievalVersion::new(wire), Ok(expected));
        assert_eq!(expected.number(), wire);
    }
    for wire in [0, 3, u16::MAX] {
        assert_eq!(
            RetrievalVersion::new(wire),
            Err(RetrievalError::UnsupportedVersion)
        );
    }
}

#[test]
fn sufficiency_audit_codes_preserve_distinct_denial_and_success_states() {
    use SufficiencyFinding::*;
    for (finding, code) in [
        (Sufficient, "SUFFICIENCY_SUFFICIENT"),
        (Partial, "SUFFICIENCY_PARTIAL"),
        (Insufficient, "SUFFICIENCY_INSUFFICIENT"),
        (Conflicting, "SUFFICIENCY_CONFLICTING"),
        (Stale, "SUFFICIENCY_STALE"),
        (Untrusted, "SUFFICIENCY_UNTRUSTED"),
        (Contaminated, "SUFFICIENCY_CONTAMINATED"),
        (BudgetExhausted, "SUFFICIENCY_BUDGET_EXHAUSTED"),
    ] {
        assert_eq!(finding.as_str(), code);
    }
}
