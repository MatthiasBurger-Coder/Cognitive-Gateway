//! Provider-neutral contracts are owned by the application boundary.
pub use gateway_application::codex::contracts::{artifact, bundled, failure, token};
use serde_json::{Value, json};

pub fn tools() -> Value {
    let catalog = artifact("catalog").unwrap();
    Value::Array(catalog["tools"].as_array().unwrap().iter().map(|tool| json!({
        "name": tool["name"], "description": format!("{}; application availability depends on admitted host services", tool["operation"].as_str().unwrap()),
        "inputSchema": bundled("request.schema.json", &tool["operation"]),
        "outputSchema": bundled("response.schema.json", &tool["operation"]),
        "annotations": tool["annotations"], "execution": tool["execution"]
    })).collect())
}

#[cfg(test)]
pub use gateway_application::codex::contracts::valid;
