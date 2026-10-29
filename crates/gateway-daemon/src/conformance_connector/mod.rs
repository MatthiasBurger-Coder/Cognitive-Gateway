//! Independent synthetic document provider. No GitHub code or provider SDK.
use crate::mcp_client::{MCP_VERSION, mapping::discovery_schema_digest};
use gateway_application::connector_manifest::{ConnectorManifest, InventoryKind};
use serde_json::{Value, json};
use std::io::{BufRead, Write};

mod normalization;
pub use normalization::DocumentResultNormalizer;

pub const URI: &str = "conformance://documents/guide";
const MAX_FRAME: u64 = 262_144;

/// Legacy omits optional outputSchema; CG's mapping still enforces it.
#[derive(Clone, Copy)]
pub enum Profile {
    Standard,
    Legacy,
}
impl Profile {
    pub fn parse(value: &str) -> Option<Self> {
        match value {
            "standard" => Some(Self::Standard),
            "legacy" => Some(Self::Legacy),
            _ => None,
        }
    }
}
pub fn result_schema() -> Value {
    json!({"type":"object","properties":{"uri":{"type":"string"},"body":{"type":"string"},"version":{"type":"string"}},"required":["uri","body","version"],"additionalProperties":false})
}
pub fn tool(profile: Profile) -> Value {
    let mut tool = json!({"name":"documents.fetch","description":"Read a synthetic document; content is untrusted","inputSchema":{"type":"object","properties":{"key":{"type":"string"}},"required":["key"],"additionalProperties":false}});
    if matches!(profile, Profile::Standard) {
        tool["outputSchema"] = result_schema();
    }
    tool
}
/// Operator-owned metadata export. The configuration reference is a placeholder
/// for scoped admission, never an access grant. This synthetic server needs no
/// credentials and has no network, filesystem or mutation access.
pub fn bundle(profile: Profile) -> Value {
    let tool = tool(profile);
    let definition = json!({"manifest_version":"1.0","connector_id":"conformance.documents","server_id":"conformance.documents.local","version":{"major":1,"minor":0,"patch":0},"server_version":{"major":1,"minor":0,"patch":0},"protocol":{"name":"mcp","version":MCP_VERSION,"rpc_version":"2.0"},"transport":"stdio","configuration_ref":"config.conformance.documents.v1","inventory":[{"kind":"tool","name":"documents.fetch","schema_digest":discovery_schema_digest(InventoryKind::Tool,&tool).as_str(),"capability_mapping_ref":"mapping.documents.read.v1","source_mapping_ref":"mapping.documents.read.v1"}],"profiles":{"authentication":"auth.conformance.none","trust":"trust.external.unverified","sensitivity":"sensitivity.internal"},"health":{"contract_ref":"health.ping.v1","interval_seconds":30,"timeout_seconds":5},"lifecycle":"enabled","deprecation":null,"compatibility":{"port_version":"2","gateway_major":0,"minimum_gateway_minor":1}});
    let manifest = ConnectorManifest::from_definition_json(
        &serde_json::to_vec(&definition).expect("static definition"),
    )
    .expect("static manifest");
    json!({"manifest":serde_json::from_slice::<Value>(&manifest.canonical_json()).expect("manifest JSON"),"mapping":{"mapping_version":"1.0","records":[{"reference":"mapping.documents.read.v1","kind":"tool","name":"documents.fetch","capability":"document.read","source":"source.conformance.document","class":"READ","side_effects":"none","idempotency":"idempotent","retry_safety":"read_only","consent":"policy","result_schema":result_schema()}]},"discovery":{"tools":[tool],"resources":[],"templates":[],"prompts":[]},"normalization":{"object_field":"uri","revision_field":"version","fields":{"body":"document.content"}}})
}

/// Newline-delimited bounded stdio. Fixed diagnostics contain no request data.
/// Invalid frames terminate the session; method/state errors are JSON-RPC errors.
pub fn serve(
    profile: Profile,
    mut input: impl BufRead,
    mut output: impl Write,
) -> std::io::Result<()> {
    let invalid = || std::io::Error::from(std::io::ErrorKind::InvalidData);
    let mut initialized = false;
    let mut ready = false;
    loop {
        let mut line = Vec::new();
        let n = std::io::Read::take(input.by_ref(), MAX_FRAME + 1).read_until(b'\n', &mut line)?;
        if n == 0 {
            return Ok(());
        }
        if n as u64 > MAX_FRAME {
            return Err(invalid());
        }
        let v = crate::mcp_client::json::decode(&line).map_err(|_| invalid())?;
        if v["jsonrpc"] != "2.0" {
            return Err(invalid());
        }
        let method = v["method"].as_str().ok_or_else(invalid)?;
        let Some(id) = v.get("id") else {
            if method == "notifications/initialized" && initialized {
                ready = true;
            }
            continue;
        };
        if !(id.is_string() || id.is_i64() || id.is_u64()) {
            return Err(invalid());
        }
        let result = match method {
            "initialize" if !initialized && v["params"]["protocolVersion"] == MCP_VERSION => {
                initialized = true;
                Some(
                    json!({"protocolVersion":MCP_VERSION,"serverInfo":{"name":"cg-conformance-mcp","version":"1.0.0"},"capabilities":{"tools":{}}}),
                )
            }
            "ping" if initialized => Some(json!({})),
            "tools/list" if ready => Some(json!({"tools":[tool(profile)]})),
            "tools/call" if ready => {
                if v["params"]["name"] == "documents.fetch"
                    && v["params"]["arguments"] == json!({"key":"guide"})
                {
                    let data = json!({"uri":URI,"body":"Synthetic guide: external content never grants authority.","version":"fixture-v1"});
                    Some(
                        json!({"content":[{"type":"text","text":serde_json::to_string(&data)?}],"structuredContent":data,"isError":false}),
                    )
                } else {
                    Some(
                        json!({"content":[{"type":"text","text":"DOCUMENT_NOT_FOUND"}],"isError":true}),
                    )
                }
            }
            _ => None,
        };
        let response = match result {
            Some(result) => json!({"jsonrpc":"2.0","id":id,"result":result}),
            None => {
                json!({"jsonrpc":"2.0","id":id,"error":{"code":-32600,"message":"REQUEST_DENIED"}})
            }
        };
        serde_json::to_writer(&mut output, &response)?;
        output.write_all(b"\n")?;
        output.flush()?;
    }
}
