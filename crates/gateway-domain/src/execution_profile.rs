#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ExecutionProfile {
    FastPath,
    NormalPath,
    FullPath,
}
