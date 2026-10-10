//! EPIC-04.03 inbound local MCP infrastructure. No provider SDK or domain mutation.
mod contracts;
pub(crate) mod decode;
pub mod transport;

use gateway_application::codex::security;
use gateway_application::codex::{CodexApplicationPort, CodexFacade};
use serde_json::{Value, json};
use std::collections::BTreeSet;
use std::time::Duration;
use transport::{Transport, TransportError};

pub const PROTOCOL_VERSION: &str = "2025-11-25";
pub const MAX_FRAME_BYTES: usize = 1_048_576;
const MAX_REQUESTS: usize = 10_000;

/// Inspect names only; no provider values or client authentication files are read.
pub fn environment_allowed(names: impl IntoIterator<Item = std::ffi::OsString>) -> bool {
    names.into_iter().all(|name| {
        name.to_str()
            .is_none_or(|name| !security::credential_name(name))
    })
}

/// Trusted launcher input, never constructed from MCP request claims.
#[derive(Debug, Clone)]
pub struct LaunchBinding {
    client_name: String,
    client_version: String,
    principal: String,
    scope: Value,
}
impl LaunchBinding {
    pub fn new(
        client_name: &str,
        client_version: &str,
        principal: &str,
        workspace: &str,
        project: &str,
        binding: &str,
    ) -> Option<Self> {
        if ![
            client_name,
            client_version,
            principal,
            workspace,
            project,
            binding,
        ]
        .iter()
        .all(|text| contracts::token(text) && !security::credential_text(text))
        {
            return None;
        }
        Some(Self {
            client_name: client_name.into(),
            client_version: client_version.into(),
            principal: principal.into(),
            scope: json!({"workspace_id":workspace,"project_id":project,"binding_id":binding}),
        })
    }
}

#[derive(Debug, PartialEq, Eq)]
enum Phase {
    New,
    Initializing,
    Ready,
    Closed,
}

pub struct Server {
    binding: LaunchBinding,
    phase: Phase,
    seen: BTreeSet<String>,
    application: Box<dyn CodexApplicationPort>,
}

fn rpc_error(id: Value, code: i32, message: &str) -> Value {
    json!({"jsonrpc":"2.0","id":id,"error":{"code":code,"message":message}})
}
fn result(id: Value, result: Value) -> Value {
    json!({"jsonrpc":"2.0","id":id,"result":result})
}
fn params_object(params: &Value, allowed: &[&str], required: &[&str]) -> bool {
    params.as_object().is_some_and(|object| {
        object.keys().all(|key| allowed.contains(&key.as_str()))
            && required.iter().all(|key| object.contains_key(*key))
    })
}
fn request_id(value: &Value) -> bool {
    value.as_str().is_some_and(|id| id.len() <= 128)
        || value
            .as_i64()
            .is_some_and(|id| id.unsigned_abs() <= 9_007_199_254_740_991)
        || value.as_u64().is_some_and(|id| id <= 9_007_199_254_740_991)
}

impl Server {
    pub fn new(binding: LaunchBinding) -> Self {
        let application = CodexFacade::unavailable(binding.scope.clone());
        Self::with_application(binding, Box::new(application))
    }

    /// Inject the shared application facade. Launch admission still owns identity.
    pub fn with_application(
        binding: LaunchBinding,
        application: Box<dyn CodexApplicationPort>,
    ) -> Self {
        Self {
            binding,
            phase: Phase::New,
            seen: BTreeSet::new(),
            application,
        }
    }

    /// All supported operations finish inline; cancellation of completed/unknown
    /// IDs is ignored. No task session is cancelled and no mutation is retried.
    pub fn handle(&mut self, frame: &[u8]) -> Option<Value> {
        let value = match decode::decode(frame) {
            Ok(value) => value,
            Err(_) => return Some(rpc_error(Value::Null, -32700, "Parse error")),
        };
        if !security::credential_free(&value) {
            // Do not echo request IDs or retain credential-bearing notifications.
            return value
                .get("id")
                .map(|_| rpc_error(Value::Null, -32600, "Invalid Request"));
        }
        let id = value.get("id").cloned();
        if !params_object(
            &value,
            &["jsonrpc", "id", "method", "params"],
            &["jsonrpc", "method"],
        ) || value["jsonrpc"] != "2.0"
            || !value["method"].is_string()
            || id.as_ref().is_some_and(|id| !request_id(id))
            || value
                .get("params")
                .is_some_and(|params| !params.is_object())
        {
            return Some(rpc_error(Value::Null, -32600, "Invalid Request"));
        }
        let method = value["method"].as_str().unwrap();
        let params = value.get("params").cloned().unwrap_or_else(|| json!({}));
        let Some(id) = id else {
            if method == "notifications/initialized"
                && self.phase == Phase::Initializing
                && params_object(&params, &[], &[])
            {
                self.phase = Phase::Ready;
            }
            // Cancellation reasons are never retained. Inline requests are already
            // complete by the time a cancellation notification is consumed.
            return None;
        };
        if self.phase == Phase::Closed {
            return None;
        }
        if self.seen.len() >= MAX_REQUESTS {
            self.phase = Phase::Closed;
            return Some(rpc_error(id, -32000, "Request limit exceeded"));
        }
        if !self.seen.insert(id.to_string()) {
            return Some(rpc_error(id, -32600, "Invalid Request"));
        }
        if method == "initialize" {
            return Some(self.initialize(id, &params));
        }
        if method == "ping" {
            return Some(if params_object(&params, &[], &[]) {
                result(id, json!({}))
            } else {
                rpc_error(id, -32602, "Invalid params")
            });
        }
        if self.phase != Phase::Ready {
            return Some(rpc_error(id, -32000, "Session not initialized"));
        }
        let response = match method {
            "tools/list"
                if params_object(&params, &["cursor"], &[]) && params.get("cursor").is_none() =>
            {
                result(id, json!({"tools":contracts::tools()}))
            }
            "resources/list"
                if params_object(&params, &["cursor"], &[]) && params.get("cursor").is_none() =>
            {
                let resources: Vec<Value> = ["catalog", "common.schema.json", "request.schema.json", "response.schema.json", "resource.schema.json", "catalog.schema.json"].iter()
                    .map(|name| json!({"uri":format!("cg://contracts/1.0/{name}"),"name":name,"mimeType":"application/json"})).collect();
                result(id, json!({"resources":resources}))
            }
            "resources/templates/list" if params_object(&params, &[], &[]) => result(
                id,
                json!({"resourceTemplates":contracts::artifact("catalog").unwrap()["resources"].as_array().unwrap().iter()
                .filter(|resource| resource["kind"] == "template").map(|resource| json!({"uriTemplate":resource["uri"],"name":resource["uri"],"mimeType":"application/json"})).collect::<Vec<_>>() }),
            ),
            "tools/call" => self.call(id, &params),
            "resources/read" => self.read(id, &params),
            "tools/list" | "resources/list" | "resources/templates/list" => {
                rpc_error(id, -32602, "Invalid params")
            }
            _ => rpc_error(id, -32601, "Method not found"),
        };
        Some(response)
    }

    fn initialize(&mut self, id: Value, params: &Value) -> Value {
        if self.phase != Phase::New {
            return rpc_error(id, -32600, "Invalid Request");
        }
        if !params_object(
            params,
            &["protocolVersion", "capabilities", "clientInfo", "_meta"],
            &["protocolVersion", "capabilities", "clientInfo"],
        ) || !params["capabilities"].is_object()
            || !params["clientInfo"].is_object()
        {
            self.phase = Phase::Closed;
            return rpc_error(id, -32602, "Invalid params");
        }
        if params["protocolVersion"] != PROTOCOL_VERSION {
            self.phase = Phase::Closed;
            return rpc_error(id, -32602, "Unsupported protocol version");
        }
        if params["clientInfo"]["name"] != self.binding.client_name
            || params["clientInfo"]["version"] != self.binding.client_version
            || self.binding.principal.is_empty()
        {
            self.phase = Phase::Closed;
            return rpc_error(id, -32000, "Client admission denied");
        }
        self.phase = Phase::Initializing;
        result(
            id,
            json!({"protocolVersion":PROTOCOL_VERSION,"capabilities":{"tools":{},"resources":{}},
            "serverInfo":{"name":"cognitive-gateway","version":env!("CARGO_PKG_VERSION")},
            "instructions":"Contract discovery is available. Application tools delegate to the shared CG facade and require admitted host services; scoped resources require an admitted workspace host."}),
        )
    }

    fn call(&self, id: Value, params: &Value) -> Value {
        if !params_object(
            params,
            &["name", "arguments", "_meta"],
            &["name", "arguments"],
        ) || !params["name"].is_string()
            || !params["arguments"].is_object()
        {
            return rpc_error(id, -32602, "Invalid params");
        }
        let catalog = contracts::artifact("catalog").unwrap();
        let Some(tool) = catalog["tools"]
            .as_array()
            .unwrap()
            .iter()
            .find(|tool| tool["name"] == params["name"])
        else {
            return rpc_error(id, -32602, "Invalid params");
        };
        let request = &params["arguments"];
        let common = contracts::artifact("common.schema.json").unwrap();
        let envelope = if request["schema_version"] == "1.0"
            && request["operation"] == tool["operation"]
            && contracts::valid(
                request,
                &contracts::artifact("request.schema.json").unwrap(),
                &common,
            )
            && request["scope"] != self.binding.scope
        {
            contracts::failure("CG_SCOPE_DENIED")
        } else {
            self.application
                .execute(tool["operation"].as_str().unwrap(), request)
        };
        let envelope = if security::credential_free(&envelope)
            && (envelope["result"]["canonical_result"]["kind"] != "document"
                || (security::inline_allowed(&envelope["result"]["canonical_result"]["document"])
                    && !envelope["provenance"].as_array().is_some_and(|entries| {
                        entries.iter().any(|p| p["sensitivity"] == "SECRET")
                    })))
            && security::inline_allowed(&envelope["result"])
        {
            envelope
        } else {
            contracts::failure("CG_SENSITIVITY_DENIED")
        };
        result(
            id,
            json!({"structuredContent":envelope,"content":[{"type":"text","text":envelope.to_string()}],"isError":envelope["status"] != "ok"}),
        )
    }

    fn read(&self, id: Value, params: &Value) -> Value {
        if !params_object(params, &["uri", "_meta"], &["uri"]) || !params["uri"].is_string() {
            return rpc_error(id, -32602, "Invalid params");
        }
        let uri = params["uri"].as_str().unwrap();
        if let Some(artifact) = uri
            .strip_prefix("cg://contracts/1.0/")
            .and_then(contracts::artifact)
        {
            return result(
                id,
                json!({"contents":[{"uri":uri,"mimeType":"application/json","text":artifact.to_string()}]}),
            );
        }
        let parts: Vec<_> = uri.split('/').collect();
        if parts.len() == 12
            && parts[..3] == ["cg:", "", "workspaces"]
            && parts[4] == "projects"
            && parts[6] == "bindings"
            && parts[8] == "references"
            && [parts[3], parts[5], parts[7], parts[9], parts[10]]
                .iter()
                .all(|p| contracts::token(p))
        {
            let scope =
                json!({"workspace_id":parts[3],"project_id":parts[5],"binding_id":parts[7]});
            if scope == self.binding.scope {
                if let Ok(resource) = self
                    .application
                    .read_resource(&scope, parts[9], parts[10], parts[11])
                {
                    if security::credential_free(&resource)
                        && security::inline_allowed(&resource["document"])
                        && !resource["provenance"].as_array().is_some_and(|entries| {
                            entries.iter().any(|p| p["sensitivity"] == "SECRET")
                        })
                    {
                        return result(
                            id,
                            json!({"contents":[{"uri":uri,"mimeType":"application/json","text":resource.to_string()}]}),
                        );
                    }
                }
            }
        }
        // All rejections hide source existence and rejected URI content.
        let mut error = rpc_error(id, -32001, "Resource unavailable");
        error["error"]["data"] = contracts::failure("CG_SCOPE_DENIED")["diagnostics"][0].clone();
        error
    }

    pub fn serve(
        &mut self,
        transport: &mut impl Transport,
        read_timeout: Duration,
        write_timeout: Duration,
    ) -> Result<(), TransportError> {
        let outcome = (|| {
            while self.phase != Phase::Closed {
                let Some(frame) = transport.receive(read_timeout)? else {
                    break;
                };
                if frame.len() >= MAX_FRAME_BYTES {
                    return Err(TransportError::Limit);
                }
                if let Some(response) = self.handle(&frame) {
                    transport.send(response.to_string().into_bytes(), write_timeout)?;
                }
            }
            Ok(())
        })();
        self.phase = Phase::Closed;
        outcome
    }
}

#[cfg(test)]
mod tests;
