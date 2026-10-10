//! Shared structured-task contracts, owned by EPIC-08 (#272).
//!
//! Transport adapters supply authenticated ownership separately from commands.
//! The coordinator owns lifecycle; trusted ports supply current authority and storage.
mod admission;
mod authority;
pub mod boundary;
mod contracts;
mod coordinator;
mod journal;
mod verification;
pub use admission::*;
pub use authority::*;
pub use contracts::*;
pub use coordinator::*;
pub use journal::*;
pub use verification::*;
