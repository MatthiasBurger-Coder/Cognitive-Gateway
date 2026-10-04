use super::*;
use std::io::{self, Cursor, Read, Write};
use std::sync::{Arc, Mutex};
use transport::{StdioTransport, read_frame};

fn binding() -> LaunchBinding {
    LaunchBinding::new(
        "codex",
        "1.0",
        "operator",
        "workspace-example",
        "project-example",
        "binding-example",
    )
    .unwrap()
}
fn initialize() -> Value {
    json!({"jsonrpc":"2.0","id":1,"method":"initialize","params":{"protocolVersion":PROTOCOL_VERSION,"capabilities":{},"clientInfo":{"name":"codex","version":"1.0"}}})
}
fn handle(server: &mut Server, request: Value) -> Value {
    server.handle(request.to_string().as_bytes()).unwrap()
}
fn ready() -> Server {
    let mut server = Server::new(binding());
    assert_eq!(
        handle(&mut server, initialize())["result"]["protocolVersion"],
        PROTOCOL_VERSION
    );
    assert!(
        server
            .handle(br#"{"jsonrpc":"2.0","method":"notifications/initialized"}"#)
            .is_none()
    );
    server
}
fn request(id: u64, method: &str, params: Value) -> Value {
    json!({"jsonrpc":"2.0","id":id,"method":method,"params":params})
}

#[test]
fn lifecycle_and_admission_fail_closed() {
    assert!(LaunchBinding::new("", "1", "p", "w", "p", "b").is_none());
    assert!(LaunchBinding::new("x", "1", "p", "../w", "p", "b").is_none());
    let mut server = Server::new(binding());
    assert_eq!(
        handle(&mut server, request(2, "tools/list", json!({})))["error"]["code"],
        -32000
    );
    assert_eq!(
        handle(&mut server, request(3, "ping", json!({})))["result"],
        json!({})
    );
    assert!(
        server
            .handle(br#"{"jsonrpc":"2.0","method":"notifications/initialized"}"#)
            .is_none()
    );
    assert_eq!(server.phase, Phase::New);
    handle(&mut server, initialize());
    assert_eq!(
        handle(&mut server, request(4, "initialize", json!({})))["error"]["code"],
        -32600
    );
    assert!(
        server
            .handle(
                br#"{"jsonrpc":"2.0","method":"notifications/initialized","params":{"extra":1}}"#
            )
            .is_none()
    );
    assert_eq!(server.phase, Phase::Initializing);
    for (field, value, message) in [
        (
            "protocolVersion",
            json!("2024-11-05"),
            "Unsupported protocol version",
        ),
        (
            "clientInfo",
            json!({"name":"spoof","version":"1.0"}),
            "Client admission denied",
        ),
        (
            "clientInfo",
            json!({"name":"codex","version":"other"}),
            "Client admission denied",
        ),
        ("capabilities", json!(null), "Invalid params"),
    ] {
        let mut server = Server::new(binding());
        let mut init = initialize();
        init["params"][field] = value;
        assert_eq!(handle(&mut server, init)["error"]["message"], message);
        assert_eq!(server.phase, Phase::Closed);
        assert!(
            server
                .handle(request(2, "tools/list", json!({})).to_string().as_bytes())
                .is_none()
        );
    }
}

#[test]
fn decode_and_rpc_errors_are_sanitized() {
    let mut server = ready();
    for frame in [
        b"secret".as_slice(),
        br#"{"jsonrpc":"2.0","method":"ping","id":2,"id":3}"#,
        br#"{"jsonrpc":"2.0","method":"ping","params":{"secret":1,"secret":2},"id":2}"#,
        b"{} {}",
        &[0xff],
        b"1e400",
    ] {
        let response = server.handle(frame).unwrap();
        assert_eq!(response["error"]["code"], -32700);
        assert!(!response.to_string().contains("secret"));
    }
    let deep = format!("{}0{}", "[".repeat(66), "]".repeat(66));
    assert_eq!(
        server.handle(deep.as_bytes()).unwrap()["error"]["code"],
        -32700
    );
    for value in [
        json!([]),
        json!({"id":2,"method":"ping","jsonrpc":"1.0"}),
        json!({"id":null,"method":"ping","jsonrpc":"2.0"}),
        json!({"id":2,"method":null,"jsonrpc":"2.0"}),
        request(2, "ping", json!([])),
        request(9_007_199_254_740_992, "ping", json!({})),
        json!({"id":-9_007_199_254_740_992i64,"method":"ping","jsonrpc":"2.0"}),
        json!({"id":"x".repeat(129),"method":"ping","jsonrpc":"2.0"}),
    ] {
        assert_eq!(handle(&mut server, value)["error"]["code"], -32600);
    }
    assert_eq!(
        handle(&mut server, request(2, "unknown", json!({})))["error"]["code"],
        -32601
    );
    assert_eq!(
        handle(&mut server, request(2, "ping", json!({})))["error"]["code"],
        -32600
    );
    assert_eq!(
        handle(&mut server, request(3, "ping", json!({"extra":true})))["error"]["code"],
        -32602
    );
    // Exercise all strict JSON primitives in ignored notifications, without retaining values.
    assert!(server.handle(br#"{"jsonrpc":"2.0","method":"unknown","params":{"v":[true,false,null,-1,1,1.5,"x"]}}"#).is_none());
    server.seen = (0..MAX_REQUESTS).map(|i| i.to_string()).collect();
    assert_eq!(
        handle(&mut server, request(4, "ping", json!({})))["error"]["message"],
        "Request limit exceeded"
    );
    assert_eq!(server.phase, Phase::Closed);
}

#[test]
fn discovery_bundles_local_references_and_exposes_static_resources() {
    let mut server = ready();
    let tools = handle(&mut server, request(2, "tools/list", json!({})));
    let listed = tools["result"]["tools"].as_array().unwrap();
    assert_eq!(listed.len(), 13);
    for tool in listed {
        for schema in ["inputSchema", "outputSchema"] {
            assert!(tool[schema]["$defs"].is_object());
            assert!(!tool[schema].to_string().contains("common.schema.json#"));
        }
        assert_eq!(tool["execution"]["taskSupport"], "forbidden");
    }
    let listed = handle(&mut server, request(3, "resources/list", json!({})));
    for (index, resource) in listed["result"]["resources"]
        .as_array()
        .unwrap()
        .iter()
        .enumerate()
    {
        let read = handle(
            &mut server,
            request(
                10 + index as u64,
                "resources/read",
                json!({"uri":resource["uri"]}),
            ),
        );
        let artifact: Value =
            serde_json::from_str(read["result"]["contents"][0]["text"].as_str().unwrap()).unwrap();
        assert!(artifact.is_object());
    }
    assert_eq!(
        handle(
            &mut server,
            request(20, "resources/templates/list", json!({}))
        )["result"]["resourceTemplates"]
            .as_array()
            .unwrap()
            .len(),
        2
    );
    for (index, method) in [
        "tools/list",
        "resources/list",
        "resources/templates/list",
        "resources/read",
    ]
    .iter()
    .enumerate()
    {
        assert_eq!(
            handle(
                &mut server,
                request(30 + index as u64, method, json!({"cursor":"secret"}))
            )["error"]["code"],
            -32602
        );
    }
    for (index, uri) in [
        "cg://contracts/1.0/../request.schema.json",
        "cg://contracts/1.0/%63atalog",
        "cg://workspaces/other/projects/p/bindings/b/references/r/1/d",
        "file:///secret",
    ]
    .iter()
    .enumerate()
    {
        let response = handle(
            &mut server,
            request(40 + index as u64, "resources/read", json!({"uri":uri})),
        );
        assert_eq!(response["error"]["code"], -32001);
        assert_eq!(response["error"]["data"]["code"], "CG_SCOPE_DENIED");
        assert!(!response.to_string().contains(uri));
    }
}

#[test]
fn every_frozen_request_returns_contract_failure_without_dispatch() {
    let mut server = ready();
    let fixtures =
        std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../tests/fixtures/codex-v1");
    let mut id = 2;
    for tool in contracts::artifact("catalog").unwrap()["tools"]
        .as_array()
        .unwrap()
    {
        let operation = tool["operation"].as_str().unwrap();
        let envelope: Value = serde_json::from_slice(
            &std::fs::read(fixtures.join(format!("{operation}.request.json"))).unwrap(),
        )
        .unwrap();
        let response = handle(
            &mut server,
            request(
                id,
                "tools/call",
                json!({"name":tool["name"],"arguments":envelope}),
            ),
        );
        id += 1;
        assert_eq!(
            response["result"]["structuredContent"]["diagnostics"][0]["code"],
            "CG_UNSUPPORTED_CAPABILITY",
            "{operation}"
        );
        assert_eq!(
            serde_json::from_str::<Value>(
                response["result"]["content"][0]["text"].as_str().unwrap()
            )
            .unwrap(),
            response["result"]["structuredContent"]
        );
        assert_eq!(response["result"]["isError"], true);
    }
    let envelope: Value = serde_json::from_str(include_str!(
        "../../../../tests/fixtures/codex-v1/registry.inspect.request.json"
    ))
    .unwrap();
    for (field, value, code) in [
        ("schema_version", json!("2.0"), "CG_UNSUPPORTED_VERSION"),
        ("operation", json!("state.explain"), "CG_INVALID_REQUEST"),
        (
            "scope",
            json!({"workspace_id":"other","project_id":"project-example","binding_id":"binding-example"}),
            "CG_SCOPE_DENIED",
        ),
        (
            "input",
            json!({"kind":"agent","ids":[],"api_key":"secret"}),
            "CG_INVALID_REQUEST",
        ),
        ("execution", json!({}), "CG_INVALID_REQUEST"),
    ] {
        let mut envelope = envelope.clone();
        envelope[field] = value;
        let response = handle(
            &mut server,
            request(
                id,
                "tools/call",
                json!({"name":"cg_registry_inspect_v1","arguments":envelope}),
            ),
        );
        id += 1;
        assert_eq!(
            response["result"]["structuredContent"]["diagnostics"][0]["code"],
            code
        );
        assert!(!response.to_string().contains("secret"));
    }
    for params in [
        json!({}),
        json!({"name":"missing","arguments":{}}),
        json!({"name":"cg_registry_inspect_v1","arguments":{},"task":{}}),
    ] {
        assert_eq!(
            handle(&mut server, request(id, "tools/call", params))["error"]["code"],
            -32602
        );
        id += 1;
    }
    for target in [1, 2, 999] {
        assert!(server.handle(json!({"jsonrpc":"2.0","method":"notifications/cancelled","params":{"requestId":target,"reason":"secret"}}).to_string().as_bytes()).is_none());
    }
    assert_eq!(
        handle(&mut server, request(id, "ping", json!({})))["result"],
        json!({})
    );
}

struct Fake {
    input: Vec<Vec<u8>>,
    output: Vec<Vec<u8>>,
    read_error: Option<TransportError>,
    write_error: Option<TransportError>,
}
impl Transport for Fake {
    fn receive(&mut self, _: Duration) -> Result<Option<Vec<u8>>, TransportError> {
        if let Some(error) = self.read_error.take() {
            return Err(error);
        }
        Ok(if self.input.is_empty() {
            None
        } else {
            Some(self.input.remove(0))
        })
    }
    fn send(&mut self, frame: Vec<u8>, _: Duration) -> Result<(), TransportError> {
        if let Some(error) = self.write_error.take() {
            return Err(error);
        }
        self.output.push(frame);
        Ok(())
    }
}
#[test]
fn transport_replacement_eof_and_failures_close_session() {
    let mut fake = Fake {
        input: vec![
            initialize().to_string().into_bytes(),
            br#"{"jsonrpc":"2.0","method":"notifications/initialized"}"#.to_vec(),
            request(2, "ping", json!({})).to_string().into_bytes(),
        ],
        output: vec![],
        read_error: None,
        write_error: None,
    };
    let mut server = Server::new(binding());
    server
        .serve(&mut fake, Duration::ZERO, Duration::ZERO)
        .unwrap();
    assert_eq!(fake.output.len(), 2);
    assert_eq!(server.phase, Phase::Closed);
    for (read_error, write_error, input) in [
        (Some(TransportError::Timeout), None, vec![]),
        (Some(TransportError::Io), None, vec![]),
        (
            None,
            Some(TransportError::Io),
            vec![initialize().to_string().into_bytes()],
        ),
        (None, None, vec![vec![b'x'; MAX_FRAME_BYTES]]),
    ] {
        let mut fake = Fake {
            input,
            output: vec![],
            read_error,
            write_error,
        };
        let mut server = Server::new(binding());
        assert!(
            server
                .serve(&mut fake, Duration::ZERO, Duration::ZERO)
                .is_err()
        );
        assert_eq!(server.phase, Phase::Closed);
    }
    let mut init = initialize();
    init["params"]["protocolVersion"] = json!("old");
    let mut fake = Fake {
        input: vec![init.to_string().into_bytes()],
        output: vec![],
        read_error: None,
        write_error: None,
    };
    Server::new(binding())
        .serve(&mut fake, Duration::ZERO, Duration::ZERO)
        .unwrap();
    assert_eq!(fake.output.len(), 1);
}

#[derive(Clone)]
struct Output(Arc<Mutex<Vec<u8>>>);
impl Write for Output {
    fn write(&mut self, data: &[u8]) -> io::Result<usize> {
        self.0.lock().unwrap().extend_from_slice(data);
        Ok(data.len())
    }
    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}
struct Broken;
impl Read for Broken {
    fn read(&mut self, _: &mut [u8]) -> io::Result<usize> {
        Err(io::Error::other("secret"))
    }
}
impl Write for Broken {
    fn write(&mut self, _: &[u8]) -> io::Result<usize> {
        Err(io::Error::other("secret"))
    }
    fn flush(&mut self) -> io::Result<()> {
        Err(io::Error::other("secret"))
    }
}
struct Blocked;
impl Read for Blocked {
    fn read(&mut self, _: &mut [u8]) -> io::Result<usize> {
        std::thread::park();
        Ok(0)
    }
}
impl Write for Blocked {
    fn write(&mut self, _: &[u8]) -> io::Result<usize> {
        std::thread::park();
        Ok(0)
    }
    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}
#[test]
fn stdio_framing_io_limits_and_blocked_pipes_are_bounded() {
    let wait = Duration::from_secs(1);
    assert_eq!(read_frame(&mut Cursor::new(b""), 10), Ok(None));
    assert_eq!(
        read_frame(&mut Cursor::new(b"{}\n"), 3),
        Ok(Some(b"{}".to_vec()))
    );
    assert_eq!(
        read_frame(&mut Cursor::new(b"{}"), 3),
        Err(TransportError::Frame)
    );
    assert_eq!(
        read_frame(&mut Cursor::new(b"1234\n"), 3),
        Err(TransportError::Limit)
    );
    assert_eq!(
        read_frame(&mut io::BufReader::new(Broken), 3),
        Err(TransportError::Io)
    );
    let output = Output(Arc::new(Mutex::new(vec![])));
    let mut transport = StdioTransport::new(Cursor::new(b"{}\n"), output.clone(), 16);
    assert_eq!(transport.receive(wait), Ok(Some(b"{}".to_vec())));
    assert_eq!(transport.receive(wait), Ok(None));
    assert_eq!(transport.receive(wait), Err(TransportError::Io));
    transport.send(b"{}".to_vec(), wait).unwrap();
    assert_eq!(*output.0.lock().unwrap(), b"{}\n");
    assert_eq!(
        transport.send(vec![0; 16], wait),
        Err(TransportError::Limit)
    );
    let mut transport = StdioTransport::new(Broken, Broken, 16);
    assert_eq!(transport.receive(wait), Err(TransportError::Io));
    assert_eq!(
        transport.send(b"{}".to_vec(), wait),
        Err(TransportError::Io)
    );
    // Worker has exited: a second send cannot revive the broken stream.
    assert_eq!(
        transport.send(b"{}".to_vec(), wait),
        Err(TransportError::Io)
    );
    let mut transport = StdioTransport::new(Blocked, Blocked, 16);
    let deadline = Duration::from_millis(10);
    assert_eq!(transport.receive(deadline), Err(TransportError::Timeout));
    assert_eq!(
        transport.send(b"{}".to_vec(), deadline),
        Err(TransportError::Timeout)
    );
}

#[test]
fn frozen_schema_validator_rejects_invalid_boundary_inputs() {
    let common = contracts::artifact("common.schema.json").unwrap();
    let cases = [
        (json!("x"), json!({"type":"unknown"})),
        (json!(true), json!({"type":"string"})),
        (json!(null), json!({"const":1})),
        (json!("x"), json!({"enum":["y"]})),
        (json!(-1), json!({"type":"integer","minimum":0})),
        (json!(10), json!({"maximum":9})),
        (json!("x"), json!({"minLength":2})),
        (json!("xy"), json!({"maxLength":1})),
        (json!("x"), json!({"pattern":"unknown"})),
        (
            json!("/x"),
            json!({"pattern":"^[A-Za-z0-9][A-Za-z0-9._:-]{0,127}$"}),
        ),
        (json!({}), json!({"required":["x"]})),
        (json!({"x":1}), json!({"additionalProperties":false})),
        (json!([1, 1]), json!({"uniqueItems":true})),
        (json!([1]), json!({"maxItems":0})),
        (json!([]), json!({"minItems":1})),
        (json!([1]), json!({"items":{"type":"string"}})),
        (json!(1), json!({"anyOf":[{"const":2}]})),
        (json!(1), json!({"oneOf":[{"const":1},{"const":1}]})),
        (json!(1), json!({"$ref":"missing"})),
        (json!(1), json!({"$ref":"common.schema.json#/missing"})),
        (json!(1), json!({"if":{"const":2},"else":{"const":3}})),
    ];
    for (value, schema) in cases {
        assert!(
            !contracts::valid(&value, &schema, &common),
            "{value} {schema}"
        );
    }
    for (value, schema) in [
        (json!(true), json!({"type":"boolean"})),
        (json!(null), json!({"type":"null"})),
        (json!(1), json!({"anyOf":[{"const":1}]})),
    ] {
        assert!(contracts::valid(&value, &schema, &common));
    }
    let reference = common["$defs"]["reference"].clone();
    let mut example: Value = serde_json::from_str(include_str!(
        "../../../../tests/fixtures/codex-v1/state.explain.request.json"
    ))
    .unwrap();
    example["input"]["resolution"]["digest"] = json!("sha256:invalid");
    assert!(!contracts::valid(
        &example["input"]["resolution"],
        &reference,
        &common
    ));
    assert!(contracts::artifact("../../secret").is_none());
}
