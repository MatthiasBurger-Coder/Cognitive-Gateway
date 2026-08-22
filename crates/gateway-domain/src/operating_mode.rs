#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OperatingMode {
    Development,
    Hardening,
    ReleaseQualification,
}
