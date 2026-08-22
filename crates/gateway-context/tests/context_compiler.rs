use gateway_context::ContextCompiler;
use gateway_domain::{
    TaskId, execution_context::ExecutionContext, execution_profile::ExecutionProfile,
    operating_mode::OperatingMode, task::TaskDescriptor,
};

fn task() -> TaskDescriptor {
    TaskDescriptor::new(
        TaskId::new("context-test-task").unwrap(),
        "Verify context compilation",
    )
    .unwrap()
}

#[test]
fn compiles_a_valid_task_into_an_execution_context() {
    let context = ContextCompiler::compile(
        task(),
        OperatingMode::Development,
        ExecutionProfile::NormalPath,
    );

    assert_eq!(context.task.id().as_str(), "context-test-task");
    assert_eq!(context.task.intent(), "Verify context compilation");
    assert_eq!(context.operating_mode, OperatingMode::Development);
    assert_eq!(context.execution_profile, ExecutionProfile::NormalPath);
}

#[test]
fn operating_modes_and_execution_profiles_are_independent() {
    let modes = [
        OperatingMode::Development,
        OperatingMode::Hardening,
        OperatingMode::ReleaseQualification,
    ];
    let profiles = [
        ExecutionProfile::FastPath,
        ExecutionProfile::NormalPath,
        ExecutionProfile::FullPath,
    ];

    for mode in modes {
        for profile in profiles {
            let context = ContextCompiler::compile(task(), mode, profile);

            assert_eq!(context.operating_mode, mode);
            assert_eq!(context.execution_profile, profile);
        }
    }
}

#[test]
fn compiled_context_has_the_expected_domain_type() {
    let context: ExecutionContext =
        ContextCompiler::compile(task(), OperatingMode::Hardening, ExecutionProfile::FullPath);

    assert_eq!(context.task.id().as_str(), "context-test-task");
}
