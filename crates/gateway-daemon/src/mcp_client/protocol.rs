use super::{
    transport::{Transport, TransportFactory},
    *,
};
use serde_json::json;
use std::collections::BTreeSet;

pub(super) struct Session {
    pub state: SessionState,
    pub inventory: Option<ProtocolInventory>,
    pub limits: Limits,
    transport: Option<Box<dyn Transport>>,
    next_id: u64,
    capabilities: Value,
    observer: Arc<dyn Observer>,
    pub sent: bool,
    /// Monotonic attempted-I/O counter. Marking before transport send conservatively
    /// includes partial writes and transport-side rejection; it is never reset.
    pub send_attempts: u64,
    server_version: String,
}
impl Session {
    pub fn new(limits: Limits, observer: Arc<dyn Observer>, server_version: String) -> Self {
        Self {
            state: SessionState::Disconnected,
            inventory: None,
            limits,
            transport: None,
            next_id: 1,
            capabilities: Value::Null,
            observer,
            sent: false,
            send_attempts: 0,
            server_version,
        }
    }
    fn event(&self, kind: EventKind, id: Option<u64>, context: &CallContext) {
        self.observer.record(Event {
            kind,
            request_id: id,
            correlation: context.correlation.clone(),
        });
    }
    fn transition(&mut self, state: SessionState, context: &CallContext) {
        self.state = state;
        self.event(EventKind::State(state), None, context);
    }
    pub fn close(&mut self, state: SessionState, context: &CallContext) {
        self.transition(SessionState::Closing, context);
        if let Some(mut transport) = self.transport.take() {
            transport.close();
        }
        self.inventory = None;
        self.capabilities = Value::Null;
        self.transition(state, context);
    }
    pub fn failed(&mut self, error: Error, context: &CallContext) {
        self.event(EventKind::Failed(error), None, context);
        self.close(SessionState::Degraded, context);
    }
    pub fn connect(
        &mut self,
        factory: &dyn TransportFactory,
        context: &CallContext,
    ) -> Result<(), Error> {
        if self.state == SessionState::Disabled {
            return Err(Error::InvalidState);
        }
        if self.state == SessionState::Ready {
            return context.check();
        }
        self.transition(SessionState::Connecting, context);
        let result = self.initialize(factory, context);
        if let Err(error) = result {
            self.failed(error, context);
        }
        result
    }
    fn initialize(
        &mut self,
        factory: &dyn TransportFactory,
        context: &CallContext,
    ) -> Result<(), Error> {
        context.check()?;
        self.transport = Some(factory.open(context, &self.limits)?);
        self.transition(SessionState::Initializing, context);
        context.check()?;
        let result = self.rpc(
            "initialize",
            json!({"protocolVersion":MCP_VERSION,"capabilities":{},
            "clientInfo":{"name":"cognitive-gateway","version":env!("CARGO_PKG_VERSION")}}),
            context,
        )?;
        if result.get("protocolVersion").and_then(Value::as_str) != Some(MCP_VERSION) {
            return Err(Error::IncompatibleVersion);
        }
        let info = &result["serverInfo"];
        if info["name"].as_str().is_none_or(str::is_empty)
            || !info["version"].is_string()
            || !result["capabilities"].is_object()
        {
            return Err(Error::Protocol);
        }
        if info["version"].as_str() != Some(&self.server_version) {
            return Err(Error::IncompatibleVersion);
        }
        for name in ["tools", "resources", "prompts"] {
            if result["capabilities"]
                .get(name)
                .is_some_and(|v| !v.is_object())
            {
                return Err(Error::Protocol);
            }
        }
        self.capabilities = result["capabilities"].clone();
        self.send(
            json!({"jsonrpc":"2.0","method":"notifications/initialized"}),
            context,
        )?;
        self.transition(SessionState::Ready, context);
        Ok(())
    }
    fn send(&mut self, value: Value, context: &CallContext) -> Result<(), Error> {
        context.check()?;
        let bytes = serde_json::to_vec(&value).map_err(|_| Error::Protocol)?;
        if bytes.len() > self.limits.frame_bytes {
            return Err(Error::Limit);
        }
        let transport = self.transport.as_mut().ok_or(Error::InvalidState)?;
        self.send_attempts = self.send_attempts.checked_add(1).ok_or(Error::Limit)?;
        transport.send(&bytes, context)
    }
    pub fn rpc(
        &mut self,
        method: &str,
        params: Value,
        context: &CallContext,
    ) -> Result<Value, Error> {
        self.sent = false;
        context.check()?;
        let id = self.next_id;
        self.next_id = self.next_id.checked_add(1).ok_or(Error::Limit)?;
        let bytes =
            serde_json::to_vec(&json!({"jsonrpc":"2.0","id":id,"method":method,"params":params}))
                .map_err(|_| Error::Protocol)?;
        if bytes.len() > self.limits.frame_bytes {
            return Err(Error::Limit);
        }
        context.check()?;
        let transport = self.transport.as_mut().ok_or(Error::InvalidState)?;
        // Mark before write: partial writes can leave the effect outcome unknown.
        self.send_attempts = self.send_attempts.checked_add(1).ok_or(Error::Limit)?;
        self.sent = true;
        transport.send(&bytes, context)?;
        self.event(EventKind::Sent, Some(id), context);
        let result = self.response(id, context);
        if matches!(result, Err(Error::Timeout | Error::Cancelled)) && method != "initialize" {
            let cleanup = CallContext::new(
                context.correlation.clone(),
                Instant::now() + self.limits.shutdown,
            );
            let _ = self.send(json!({"jsonrpc":"2.0","method":"notifications/cancelled","params":{"requestId":id}}), &cleanup);
        }
        if result.is_ok() {
            self.event(EventKind::Completed, Some(id), context);
        }
        result
    }
    fn response(&mut self, id: u64, context: &CallContext) -> Result<Value, Error> {
        self.response_with_wait_deadline(id, context, None)
    }
    fn response_with_wait_deadline(
        &mut self,
        id: u64,
        context: &CallContext,
        wait_deadline: Option<Instant>,
    ) -> Result<Value, Error> {
        for _ in 0..self.limits.events_per_request {
            let transport = self.transport.as_mut().ok_or(Error::InvalidState)?;
            let frame = match wait_deadline {
                Some(deadline) => transport.receive_with_wait_deadline(context, deadline)?,
                None => transport.receive(context)?,
            };
            // A short inventory-drain poll only bounds waiting for a frame.
            // Once received, process it under the live operation context so a
            // just-expired poll cannot consume and discard an invalidation.
            context.check()?;
            if frame.len() > self.limits.frame_bytes {
                return Err(Error::Limit);
            }
            let value = json::decode(&frame).map_err(|_| Error::Protocol)?;
            if !value.is_object() || value["jsonrpc"] != "2.0" {
                return Err(Error::Protocol);
            }
            if let Some(method) = value.get("method") {
                let method = method.as_str().ok_or(Error::Protocol)?;
                if value.get("result").is_some() || value.get("error").is_some() {
                    return Err(Error::Protocol);
                }
                if let Some(server_id) = value.get("id") {
                    if !(server_id.is_string() || server_id.is_i64() || server_id.is_u64()) {
                        return Err(Error::Protocol);
                    }
                    let response = if method == "ping" {
                        json!({"result":{}})
                    } else {
                        json!({"error":{"code":-32601,"message":"Unsupported method"}})
                    };
                    let mut response = response;
                    response["jsonrpc"] = json!("2.0");
                    response["id"] = server_id.clone();
                    self.send(response, context)?;
                } else if method == "notifications/cancelled"
                    && value["params"]["requestId"].as_u64() == Some(id)
                {
                    return Err(Error::Cancelled);
                } else if matches!(
                    method,
                    "notifications/tools/list_changed"
                        | "notifications/resources/list_changed"
                        | "notifications/prompts/list_changed"
                ) {
                    self.inventory = None;
                    self.event(EventKind::Invalidated, Some(id), context);
                    return Err(Error::InventoryChanged);
                }
                continue;
            }
            if value["id"].as_u64() != Some(id)
                || value.get("result").is_some() == value.get("error").is_some()
            {
                return Err(Error::Protocol);
            }
            if let Some(error) = value.get("error") {
                let code = error["code"].as_i64().ok_or(Error::Protocol)?;
                if !error["message"].is_string() {
                    return Err(Error::Protocol);
                }
                return Err(Error::Remote(code));
            }
            return value
                .get("result")
                .filter(|v| v.is_object())
                .cloned()
                .ok_or(Error::Protocol);
        }
        Err(Error::Limit)
    }
    /// Drain already queued notifications before authorizing an effect. Reserved
    /// ID zero is never issued; an unsolicited response is a protocol failure.
    pub fn refresh(&mut self, context: &CallContext) -> Result<(), Error> {
        self.ready()?;
        let mut poll = context.clone();
        poll.deadline = context
            .deadline
            .min(Instant::now() + Duration::from_millis(1));
        match self.response_with_wait_deadline(0, context, Some(poll.deadline)) {
            Err(Error::Timeout) => context.check(),
            Err(error) => Err(error),
            Ok(_) => Err(Error::Protocol),
        }
    }
    pub fn ready(&self) -> Result<(), Error> {
        if self.state == SessionState::Ready {
            Ok(())
        } else {
            Err(Error::InvalidState)
        }
    }
    pub fn discover(&mut self, context: &CallContext) -> Result<ProtocolInventory, Error> {
        self.ready()?;
        self.inventory = None;
        let mut inventory = ProtocolInventory::default();
        let mut bytes = 0;
        let mut items = 0;
        for (capability, method, key, output) in [
            ("tools", "tools/list", "tools", &mut inventory.tools),
            (
                "resources",
                "resources/list",
                "resources",
                &mut inventory.resources,
            ),
            (
                "resources",
                "resources/templates/list",
                "resourceTemplates",
                &mut inventory.templates,
            ),
            ("prompts", "prompts/list", "prompts", &mut inventory.prompts),
        ] {
            if self.capabilities.get(capability).is_none() {
                continue;
            }
            let mut cursor = None;
            let mut cursors = BTreeSet::new();
            let mut names = BTreeSet::new();
            for page in 0..self.limits.pages {
                let result = self.rpc(
                    method,
                    cursor.map_or_else(|| json!({}), |c| json!({"cursor":c})),
                    context,
                )?;
                bytes += serde_json::to_vec(&result)
                    .map_err(|_| Error::Protocol)?
                    .len();
                let entries = result[key].as_array().ok_or(Error::Protocol)?;
                items += entries.len();
                if bytes > self.limits.inventory_bytes || items > self.limits.items {
                    return Err(Error::Limit);
                }
                let identity = match key {
                    "resources" => "uri",
                    "resourceTemplates" => "uriTemplate",
                    _ => "name",
                };
                for entry in entries {
                    let name = entry[identity]
                        .as_str()
                        .filter(|s| !s.is_empty())
                        .ok_or(Error::Protocol)?;
                    if !names.insert(name.to_owned()) {
                        return Err(Error::Schema);
                    }
                    if key == "tools" && !entry["inputSchema"].is_object() {
                        return Err(Error::Schema);
                    }
                    output.push(entry.clone());
                }
                match result.get("nextCursor") {
                    None => break,
                    Some(Value::String(next))
                        if !next.is_empty()
                            && cursors.insert(next.clone())
                            && page + 1 < self.limits.pages =>
                    {
                        cursor = Some(next.clone())
                    }
                    _ => return Err(Error::Limit),
                }
            }
            output.sort_by(|a, b| {
                a[identity_key(key)]
                    .as_str()
                    .cmp(&b[identity_key(key)].as_str())
            });
        }
        self.inventory = Some(inventory.clone());
        Ok(inventory)
    }
}
fn identity_key(key: &str) -> &str {
    match key {
        "resources" => "uri",
        "resourceTemplates" => "uriTemplate",
        _ => "name",
    }
}

#[cfg(test)]
#[path = "protocol_tests.rs"]
mod tests;

#[cfg(test)]
#[path = "protocol_guard_tests.rs"]
mod guard_tests;

#[cfg(all(test, unix))]
#[path = "protocol_polling_tests.rs"]
mod polling_tests;
