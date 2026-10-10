//! Shared structured-task contracts, owned by EPIC-08 (#272).
//!
//! Transport adapters supply authenticated ownership separately from commands.
//! These contracts do not enable a runtime or turn client claims into authority.
mod admission;
mod authority;
mod contracts;
mod journal;
pub use admission::*;
pub use authority::*;
pub use contracts::*;
pub use journal::*;
