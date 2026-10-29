#![forbid(unsafe_code)]

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

pub mod codex_setup;
pub mod codex_workspace;
pub mod local_sessions;
pub mod session_store;

pub mod prompt_renderers;

pub mod model_adapters;

pub mod invocation_host;

/// Outbound MCP client; separate from the inbound Codex server.
pub mod mcp_client;

/// Optional outer GitHub reference MCP server; no provider types enter core.
pub mod github_connector;

// Shared native test data and the one actual coordinator allocation harness.
#[cfg(test)]
#[path = "../../gateway-application/tests/support/goal_observation_harness.rs"]
pub(crate) mod goal_observation_test_harness;
#[cfg(test)]
pub(crate) mod native_policy_test_support;

#[cfg(test)]
#[path = "../../gateway-application/tests/support/complete_scoped_snapshot_fixture.rs"]
pub(crate) mod complete_snapshot_fixture;
#[cfg(test)]
#[path = "../../gateway-application/tests/support/prepared_before_fixture.rs"]
pub(crate) mod prepared_before_fixture;

/// Independent synthetic MCP reference server and explicit document projection.
pub mod conformance_connector;
