use gateway_context::{ContextCompiler, ContextHandoff};
use gateway_domain::{ExecutionContextIRV2, ExecutionProjectionStatus};

#[test]
fn cg10_accepts_v2_without_downgrading_non_executable_results() {
    let handoff = ContextHandoff::V2(
        ExecutionContextIRV2::new("handoff", "basis", ExecutionProjectionStatus::NoTemplate)
            .unwrap(),
    );
    let accepted = ContextCompiler::inspect_handoff(handoff).unwrap();
    assert!(matches!(accepted, ContextHandoff::V2(_)));
}
