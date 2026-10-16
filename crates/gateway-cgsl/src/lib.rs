//! Offline CGSL frontend. Compilation produces interpretation inputs, never execution authority.
mod compiler;
mod parser;
pub use compiler::*;
pub use parser::*;
