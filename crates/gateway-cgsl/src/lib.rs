//! Offline CGSL frontend. Compilation produces interpretation inputs, never execution authority.
mod compiler;
mod parser;
mod serialization;
pub use compiler::*;
pub use parser::*;
pub use serialization::*;
