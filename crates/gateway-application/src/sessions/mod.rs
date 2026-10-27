//! Shared structured-task contracts, owned by EPIC-08 (#272).
//!
//! Transport adapters supply authenticated ownership separately from commands.
//! The coordinator owns lifecycle; trusted ports supply current authority and storage.
mod admission;
mod authority;
pub mod boundary;
mod connector_execution;
mod connector_goals;
mod contracts;
mod coordinator;
mod journal;
mod pre_execution_goal;
mod readback_storage;
mod resources;
mod verification;
pub use admission::*;
pub use authority::*;
pub use connector_execution::*;
pub use connector_goals::*;
pub use contracts::*;
pub use coordinator::*;
pub use journal::*;
pub use pre_execution_goal::*;
pub use readback_storage::*;
pub use resources::*;
pub use verification::*;
