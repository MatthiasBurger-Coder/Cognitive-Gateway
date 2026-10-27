#![forbid(unsafe_code)]

#[cfg(test)]
#[allow(dead_code)]
#[path = "../tests/support/session_resource_allocation.rs"]
mod session_resource_test_support;

#[cfg(test)]
#[allow(dead_code)]
#[path = "../../../tests/fixtures/prompt-rendering-v1/support.rs"]
mod session_model_test_support;

mod codex_canonical;

pub mod declarative_cli;
pub mod experience_patterns;
pub mod graph_retrieval;
pub mod memory;
pub mod postgres_experience;
pub mod postgres_memory;
pub mod procedure_promotion_store;
pub mod registry_cli;
pub mod retrieval;

pub mod local_inference;

pub mod worker_fabric;

pub mod bounded_process;
pub mod cognitive_store;
pub mod cpu_learning;
pub mod durable_models;
pub mod durable_workers;

pub mod local_mcp;

pub mod codex_workspace;
pub mod local_sessions;
pub mod session_store;

pub mod prompt_renderers;

pub mod model_adapters;

pub mod invocation_host;

/// Outbound MCP client; separate from the inbound Codex server.
pub mod mcp_client;
