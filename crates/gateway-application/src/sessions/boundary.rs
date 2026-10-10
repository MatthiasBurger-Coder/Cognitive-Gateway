//! Explicit v2 envelope projection of the shared typed API. Frozen v1 is separate.
use super::*;
use serde_json::{Value, json};
pub fn artifact(name: &str) -> Option<Value> {
    let source = match name {
        "catalog" => include_str!("../../../../schemas/codex/v2/catalog.json"),
        "common.schema.json" => include_str!("../../../../schemas/codex/v2/common.schema.json"),
        "request.schema.json" => include_str!("../../../../schemas/codex/v2/request.schema.json"),
        "response.schema.json" => include_str!("../../../../schemas/codex/v2/response.schema.json"),
        "resource.schema.json" => include_str!("../../../../schemas/codex/v2/resource.schema.json"),
        "catalog.schema.json" => include_str!("../../../../schemas/codex/v2/catalog.schema.json"),
        _ => return None,
    };
    serde_json::from_str(source).ok()
}
pub fn failure(code: &str) -> Value {
    json!({"schema_version":"2.0","status":"error","diagnostics":[{"code":code}]})
}
pub fn tools() -> Vec<Value> {
    artifact("catalog").expect("bundled catalog")["tools"].as_array().expect("tools").iter().map(|tool|{
        let operation=&tool["operation"];
        let mut input=artifact("request.schema.json").expect("request schema");
        input["oneOf"].as_array_mut().expect("request variants").retain(|variant|variant["properties"]["operation"]["const"]==*operation);
        json!({"name":tool["name"],"description":format!("{}; shared durable context-artifact session",operation.as_str().expect("operation")),
            "inputSchema":input,"outputSchema":artifact("response.schema.json").expect("response schema"),"annotations":tool["annotations"],"execution":tool["execution"]})
    }).collect()
}
pub enum Decoded {
    Command(SessionCommand),
    Inspect(InspectTarget),
}
pub fn decode(operation: &str, request: &Value) -> Result<Decoded, &'static str> {
    if request["schema_version"] != "2.0" {
        return Err("CG_UNSUPPORTED_VERSION");
    }
    let common = artifact("common.schema.json").ok_or("CG_INTERNAL_ERROR")?;
    if request["operation"] != operation
        || !crate::codex::contracts::valid(
            request,
            &artifact("request.schema.json").ok_or("CG_INTERNAL_ERROR")?,
            &common,
        )
    {
        return Err("CG_INVALID_INPUT");
    }
    if !crate::codex::security::credential_free(request) {
        return Err("CG_SENSITIVITY_DENIED");
    }
    let input = &request["input"];
    let id = |key: &str| input[key].as_str().ok_or("CG_INVALID_INPUT");
    let map = |e: SessionError| e.code();
    if operation == "session.inspect" {
        return Ok(Decoded::Inspect(if input.get("session_id").is_some() {
            InspectTarget::Session(SessionId::new(id("session_id")?).map_err(map)?)
        } else {
            InspectTarget::Command(CommandId::new(id("command_id")?).map_err(map)?)
        }));
    }
    let command = CommandId::new(id("command_id")?).map_err(map)?;
    let execution = execution(request)?;
    if operation == "session.start" {
        let intent = gateway_domain::Intent::from_json(&input["intent"]["document"].to_string())
            .map_err(|_| "CG_INVALID_INPUT")?;
        return Ok(Decoded::Command(SessionCommand::Start {
            command,
            intent,
            execution,
        }));
    }
    let at = Mutation {
        session: SessionId::new(id("session_id")?).map_err(map)?,
        command,
        expected_revision: Revision::new(
            input["expected_revision"]
                .as_u64()
                .ok_or("CG_INVALID_INPUT")?,
        )
        .map_err(map)?,
    };
    Ok(Decoded::Command(match operation {
        "session.clarify" => SessionCommand::Clarify {
            at,
            pending: PendingId::new(id("pending_id")?).map_err(map)?,
            answer: ClarificationAnswer::SelectSource(
                serde_json::from_value(input["answer"]["selected_source"].clone())
                    .map_err(|_| "CG_INVALID_INPUT")?,
            ),
        },
        "session.approve" => SessionCommand::Approve {
            at,
            pending: PendingId::new(id("pending_id")?).map_err(map)?,
            consent: ConsentRecordRef(
                serde_json::from_value(input["consent"]["reference"].clone())
                    .map_err(|_| "CG_INVALID_INPUT")?,
            ),
        },
        "session.continue" => SessionCommand::Continue { at },
        "session.cancel" => SessionCommand::Cancel { at },
        _ => return Err("CG_INVALID_INPUT"),
    }))
}
pub fn execution(request: &Value) -> Result<RequestedExecution, &'static str> {
    Ok(RequestedExecution {
        mode: serde_json::from_value(request["execution"]["operating_mode"].clone())
            .map_err(|_| "CG_INVALID_INPUT")?,
        profile: serde_json::from_value(request["execution"]["execution_profile"].clone())
            .map_err(|_| "CG_INVALID_INPUT")?,
    })
}
pub fn response(
    request: &Value,
    snapshot: SessionSnapshot,
    checkpoint: &SessionCheckpoint,
) -> Value {
    let interaction = match &checkpoint.pending {
        Some(PendingInteraction::Source(q)) => json!({"kind":"source","payload":q}),
        Some(PendingInteraction::Consent(a)) => json!({"kind":"consent","payload":a}),
        None => Value::Null,
    };
    let result = json!({"schema_version":"2.0","scope":request["scope"],"operation":request["operation"],"correlation":request["correlation"],
        "status":"ok","result":{"session":snapshot,"interaction":interaction,"budget":checkpoint.budget},"diagnostics":[]});
    let common = artifact("common.schema.json").expect("bundled v2 common");
    if crate::codex::security::credential_free(&result)
        && crate::codex::contracts::valid(
            &result,
            &artifact("response.schema.json").expect("bundled v2 response"),
            &common,
        )
    {
        result
    } else {
        failure("CG_INTERNAL_ERROR")
    }
}
