//! Repository-bound, read-only GitHub MCP reference server. Provider payloads
//! remain opaque content; policy, trust, acquisition time and scope belong to CG.
use crate::mcp_client::{MCP_VERSION, mapping::discovery_schema_digest};
use gateway_application::connector_manifest::{ConnectorManifest, InventoryKind};
use serde_json::{Value, json};
use std::io::{BufRead, Write};

mod http;
mod normalization;
pub use http::CurlReader;
pub use normalization::GitHubResultNormalizer;

const MAX_FRAME: usize = 262_144;
pub const MAX_BODY: usize = 98_304;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Failure {
    Configuration,
    Request,
    Unavailable,
    Authentication,
    Permission,
    NotFound,
    RateLimited,
    Payload,
}
impl Failure {
    fn code(self) -> &'static str {
        match self {
            Self::Configuration => "CONNECTOR_CONFIGURATION",
            Self::Request => "CONNECTOR_INVALID_REQUEST",
            Self::Unavailable => "CONNECTOR_UNAVAILABLE",
            Self::Authentication => "CONNECTOR_AUTH_FAILED",
            Self::Permission => "CONNECTOR_PERMISSION_DENIED",
            Self::NotFound => "CONNECTOR_NOT_FOUND",
            Self::RateLimited => "CONNECTOR_RATE_LIMITED",
            Self::Payload => "CONNECTOR_SCHEMA_MISMATCH",
        }
    }
}

/// Outer IO seam. Implementations must bound response bytes/time and hide secrets.
pub trait GitHubReader {
    fn get(&self, url: &str) -> Result<Vec<u8>, Failure>;
}

struct Route {
    name: &'static str,
    path: &'static str,
    args: &'static [(&'static str, &'static str)],
    partial: bool,
    revision: Option<&'static str>,
}
const ROUTES: &[Route] = &[
    Route {
        name: "repository.read",
        path: "",
        args: &[],
        partial: false,
        revision: Some("/updated_at"),
    },
    Route {
        name: "tree.read",
        path: "/git/trees/{ref}",
        args: &[("ref", "string")],
        partial: false,
        revision: Some("/sha"),
    },
    Route {
        name: "file.read",
        path: "/contents/{path}?ref={ref}",
        args: &[("path", "string"), ("ref", "string")],
        partial: false,
        revision: Some("/sha"),
    },
    Route {
        name: "branches.read",
        path: "/branches?per_page=30&page={page}",
        args: &[("page", "integer")],
        partial: true,
        revision: None,
    },
    Route {
        name: "commits.read",
        path: "/commits?sha={ref}&per_page=30&page={page}",
        args: &[("ref", "string"), ("page", "integer")],
        partial: true,
        revision: None,
    },
    Route {
        name: "commit.read",
        path: "/commits/{ref}",
        args: &[("ref", "string")],
        partial: true,
        revision: Some("/sha"),
    },
    Route {
        name: "issue.read",
        path: "/issues/{number}",
        args: &[("number", "integer")],
        partial: false,
        revision: Some("/updated_at"),
    },
    Route {
        name: "issue.comments",
        path: "/issues/{number}/comments?per_page=30&page={page}",
        args: &[("number", "integer"), ("page", "integer")],
        partial: true,
        revision: None,
    },
    Route {
        name: "issue.labels",
        path: "/issues/{number}/labels?per_page=30&page={page}",
        args: &[("number", "integer"), ("page", "integer")],
        partial: true,
        revision: None,
    },
    Route {
        name: "issue.subissues",
        path: "/issues/{number}/sub_issues?per_page=30&page={page}",
        args: &[("number", "integer"), ("page", "integer")],
        partial: true,
        revision: None,
    },
    Route {
        name: "issue.search",
        path: "/search/issues?q={query}&per_page=30&page={page}",
        args: &[("query", "string"), ("page", "integer")],
        partial: true,
        revision: None,
    },
    Route {
        name: "pull_request.read",
        path: "/pulls/{number}",
        args: &[("number", "integer")],
        partial: false,
        revision: Some("/updated_at"),
    },
    Route {
        name: "pull_request.files",
        path: "/pulls/{number}/files?per_page=30&page={page}",
        args: &[("number", "integer"), ("page", "integer")],
        partial: true,
        revision: None,
    },
    Route {
        name: "pull_request.reviews",
        path: "/pulls/{number}/reviews?per_page=30&page={page}",
        args: &[("number", "integer"), ("page", "integer")],
        partial: true,
        revision: None,
    },
    Route {
        name: "pull_request.comments",
        path: "/pulls/{number}/comments?per_page=30&page={page}",
        args: &[("number", "integer"), ("page", "integer")],
        partial: true,
        revision: None,
    },
    Route {
        name: "checks.read",
        path: "/commits/{ref}/check-runs?per_page=30&page={page}",
        args: &[("ref", "string"), ("page", "integer")],
        partial: true,
        revision: None,
    },
    Route {
        name: "status.read",
        path: "/commits/{ref}/status",
        args: &[("ref", "string")],
        partial: true,
        revision: Some("/sha"),
    },
    Route {
        name: "workflow.runs",
        path: "/actions/runs?per_page=30&page={page}",
        args: &[("page", "integer")],
        partial: true,
        revision: None,
    },
    Route {
        name: "workflow.read",
        path: "/actions/runs/{number}",
        args: &[("number", "integer")],
        partial: false,
        revision: Some("/updated_at"),
    },
    Route {
        name: "workflow.jobs",
        path: "/actions/runs/{number}/jobs?per_page=30&page={page}",
        args: &[("number", "integer"), ("page", "integer")],
        partial: true,
        revision: None,
    },
    Route {
        name: "workflow.job",
        path: "/actions/jobs/{number}",
        args: &[("number", "integer")],
        partial: false,
        revision: None,
    },
    Route {
        name: "workflow.artifacts",
        path: "/actions/runs/{number}/artifacts?per_page=30&page={page}",
        args: &[("number", "integer"), ("page", "integer")],
        partial: true,
        revision: None,
    },
];

pub struct GitHubConnector<R> {
    repository: String,
    reader: R,
}
fn component(s: &str) -> bool {
    !s.is_empty()
        && s.len() <= 100
        && s != "."
        && s != ".."
        && s.bytes()
            .all(|b| b.is_ascii_alphanumeric() || b"-_.".contains(&b))
}
fn encode(s: &str) -> String {
    s.bytes()
        .map(|b| {
            if b.is_ascii_alphanumeric() || b"-_.~".contains(&b) {
                (b as char).to_string()
            } else {
                format!("%{b:02X}")
            }
        })
        .collect()
}
fn result_schema() -> Value {
    json!({"type":"object","properties":{"object":{"type":"string"},"content":{"type":"string"},"revision":{"type":"string"},"partial":{"type":"boolean"}},"required":["object","content","partial"],"additionalProperties":false})
}
fn tool(route: &Route) -> Value {
    let properties: serde_json::Map<String, Value> = route
        .args
        .iter()
        .map(|(name, kind)| ((*name).into(), json!({"type":kind})))
        .collect();
    json!({"name":format!("github.{}",route.name),"inputSchema":{"type":"object","properties":properties,"required":route.args.iter().map(|(k,_)| *k).collect::<Vec<_>>(),"additionalProperties":false},"outputSchema":result_schema()})
}
impl<R: GitHubReader> GitHubConnector<R> {
    pub fn new(repository: &str, reader: R) -> Result<Self, Failure> {
        let parts: Vec<_> = repository.split('/').collect();
        if parts.len() != 2 || !parts.iter().all(|s| component(s)) {
            return Err(Failure::Configuration);
        }
        Ok(Self {
            repository: repository.into(),
            reader,
        })
    }
    /// Strict issue reference resolution. Bare #N needs this configured repo;
    /// owner/repo#N and HTTPS issue URLs must match it, without query/fragment.
    pub fn resolve_issue(&self, reference: &str) -> Result<Value, Failure> {
        let number = reference
            .strip_prefix('#')
            .or_else(|| reference.strip_prefix(&format!("{}#", self.repository)))
            .or_else(|| {
                reference.strip_prefix(&format!("https://github.com/{}/issues/", self.repository))
            })
            .ok_or(Failure::Request)?;
        if number.is_empty() || !number.bytes().all(|b| b.is_ascii_digit()) {
            return Err(Failure::Request);
        }
        let number = number.parse::<i64>().map_err(|_| Failure::Request)?;
        if number <= 0 {
            return Err(Failure::Request);
        }
        Ok(json!({"capability":"github.issue.read","arguments":{"number":number}}))
    }
    pub fn tools(&self) -> Vec<Value> {
        ROUTES.iter().map(tool).collect()
    }
    /// Operator configuration export, never a policy grant. Configuration refs
    /// must be rebound and pinned by the existing scoped admission service.
    pub fn bundle(&self) -> Value {
        let tools = self.tools();
        let inventory: Vec<_> = tools.iter().map(|t| {
            let name = t["name"].as_str().expect("static name");
            json!({"kind":"tool","name":name,"schema_digest":discovery_schema_digest(InventoryKind::Tool,t).as_str(),"capability_mapping_ref":format!("mapping.{}.v1", name.replace('_', "-")),"source_mapping_ref":format!("mapping.{}.v1", name.replace('_', "-"))})
        }).collect();
        let records: Vec<_> = ROUTES.iter().map(|r| {
            let name = format!("github.{}",r.name);
            json!({"reference":format!("mapping.{}.v1", name.replace('_', "-")),"kind":"tool","name":name,"capability":name,"source":format!("source.{name}"),"class":if r.name == "issue.search" {"SEARCH"} else {"READ"},"side_effects":"none","idempotency":"idempotent","retry_safety":"read_only","consent":"policy","result_schema":result_schema()})
        }).collect();
        let definition = json!({"manifest_version":"1.0","connector_id":"github.reference","server_id":"github.reference.local","version":{"major":1,"minor":0,"patch":0},"server_version":{"major":1,"minor":0,"patch":0},"protocol":{"name":"mcp","version":MCP_VERSION,"rpc_version":"2.0"},"transport":"stdio","configuration_ref":"config.github.reference.v1","inventory":inventory,"profiles":{"authentication":"auth.github.readonly","trust":"trust.external.unverified","sensitivity":"sensitivity.internal"},"health":{"contract_ref":"health.ping.v1","interval_seconds":30,"timeout_seconds":5},"lifecycle":"enabled","deprecation":null,"compatibility":{"port_version":"2","gateway_major":0,"minimum_gateway_minor":1}});
        let manifest = ConnectorManifest::from_definition_json(
            &serde_json::to_vec(&definition).expect("static definition"),
        )
        .expect("static manifest");
        json!({"repository":self.repository,"manifest":serde_json::from_slice::<Value>(&manifest.canonical_json()).expect("manifest JSON"),"mapping":{"mapping_version":"1.0","records":records},"discovery":{"tools":tools,"resources":[],"templates":[],"prompts":[]},"normalization":{"object_field":"object","revision_field":"revision","digest_field":null,"fields":{"content":"github.content","partial":"github.partial"}}})
    }
    fn request_url(&self, name: &str, args: &Value) -> Result<(&'static Route, String), Failure> {
        let r = ROUTES
            .iter()
            .find(|r| name == format!("github.{}", r.name))
            .ok_or(Failure::Request)?;
        let args = args
            .as_object()
            .filter(|a| a.len() == r.args.len())
            .ok_or(Failure::Request)?;
        let mut path = r.path.to_owned();
        for (key, kind) in r.args {
            let value = args.get(*key).ok_or(Failure::Request)?;
            let replacement = if *kind == "integer" {
                let n = value
                    .as_i64()
                    .filter(|n| *n > 0 && (*key != "page" || *n <= 1000))
                    .ok_or(Failure::Request)?;
                n.to_string()
            } else {
                let text = value
                    .as_str()
                    .filter(|s| !s.is_empty() && s.len() <= 512 && !s.chars().any(char::is_control))
                    .ok_or(Failure::Request)?;
                if *key == "query" {
                    if !text
                        .bytes()
                        .all(|b| b.is_ascii_alphanumeric() || b" -_.".contains(&b))
                    {
                        return Err(Failure::Request);
                    }
                    encode(&format!("repo:{} is:issue \"{text}\"", self.repository))
                } else {
                    if text
                        .split('/')
                        .any(|s| s.is_empty() || s == "." || s == "..")
                    {
                        return Err(Failure::Request);
                    }
                    if *key == "path" {
                        text.split('/').map(encode).collect::<Vec<_>>().join("/")
                    } else {
                        encode(text)
                    }
                }
            };
            path = path.replace(&format!("{{{key}}}"), &replacement);
        }
        let url = if r.name == "issue.search" {
            format!("https://api.github.com{path}")
        } else {
            format!("https://api.github.com/repos/{}{path}", self.repository)
        };
        Ok((r, url))
    }
    pub fn call(&self, name: &str, args: &Value) -> Result<Value, Failure> {
        let (r, url) = self.request_url(name, args)?;
        let bytes = self.reader.get(&url)?;
        if bytes.is_empty() || bytes.len() > MAX_BODY {
            return Err(Failure::Payload);
        }
        let value: Value = crate::mcp_client::json::decode(&bytes).map_err(|_| Failure::Payload)?;
        if !value.is_object() && !value.is_array() {
            return Err(Failure::Payload);
        }
        let mut result = json!({"object":url,"content":serde_json::to_string(&value).map_err(|_| Failure::Payload)?,"partial":r.partial || value["truncated"] == true});
        if let Some(revision) = r
            .revision
            .and_then(|p| value.pointer(p))
            .and_then(Value::as_str)
            .filter(|s| !s.is_empty())
        {
            result["revision"] = json!(revision);
        }
        Ok(result)
    }
    /// MCP baseline lifecycle; no sampling, prompts, resources or effects.
    pub fn serve(&self, mut input: impl BufRead, mut output: impl Write) -> Result<(), Failure> {
        let mut initialized = false;
        let mut ready = false;
        loop {
            let mut line = Vec::new();
            let n = std::io::Read::take(input.by_ref(), MAX_FRAME as u64 + 1)
                .read_until(b'\n', &mut line)
                .map_err(|_| Failure::Unavailable)?;
            if n == 0 {
                return Ok(());
            }
            if n > MAX_FRAME {
                return Err(Failure::Payload);
            }
            let v: Value = crate::mcp_client::json::decode(&line).map_err(|_| Failure::Request)?;
            if v["jsonrpc"] != "2.0" {
                return Err(Failure::Request);
            }
            let id = v.get("id");
            let method = v["method"].as_str().ok_or(Failure::Request)?;
            if id.is_none() {
                if method == "notifications/initialized" && initialized {
                    ready = true;
                }
                continue;
            }
            let id = id.expect("checked id");
            if !(id.is_string() || id.is_i64() || id.is_u64()) {
                return Err(Failure::Request);
            }
            let result = match method {
                "initialize" if !initialized && v["params"]["protocolVersion"] == MCP_VERSION => {
                    initialized = true;
                    Ok(
                        json!({"protocolVersion":MCP_VERSION,"serverInfo":{"name":"cg-github-mcp","version":"1.0.0"},"capabilities":{"tools":{}}}),
                    )
                }
                "ping" if initialized => Ok(json!({})),
                "tools/list" if ready => Ok(json!({"tools":self.tools()})),
                "tools/call" if ready => {
                    match self.call(
                        v["params"]["name"].as_str().unwrap_or(""),
                        &v["params"]["arguments"],
                    ) {
                        Ok(data) => {
                            Ok(json!({"content":[],"structuredContent":data,"isError":false}))
                        }
                        Err(e) => {
                            Ok(json!({"content":[{"type":"text","text":e.code()}],"isError":true}))
                        }
                    }
                }
                _ => Err(Failure::Request),
            };
            let response = match result {
                Ok(r) => json!({"jsonrpc":"2.0","id":id,"result":r}),
                Err(e) => {
                    json!({"jsonrpc":"2.0","id":id,"error":{"code":-32600,"message":e.code()}})
                }
            };
            serde_json::to_writer(&mut output, &response).map_err(|_| Failure::Unavailable)?;
            output
                .write_all(b"\n")
                .and_then(|_| output.flush())
                .map_err(|_| Failure::Unavailable)?;
        }
    }
}

#[cfg(test)]
mod tests;
