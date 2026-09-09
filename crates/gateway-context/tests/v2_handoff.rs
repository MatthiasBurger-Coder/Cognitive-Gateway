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
    let rejected = ContextCompiler::adapt_executable_v2(match accepted {
        ContextHandoff::V2(value) => value,
        ContextHandoff::V1(_) => unreachable!(),
    });
    assert!(rejected.is_err());
}
