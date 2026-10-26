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

mod connector_goals;
mod goal_observation;
mod pre_execution_goal;
mod pre_execution_source;
mod resources;
pub mod scoped_snapshots;
pub use connector_goals::*;
pub use goal_observation::*;
pub use pre_execution_goal::*;
pub use pre_execution_source::{ConnectorRequestIdentity, connector_request_digest};
pub use resources::*;
