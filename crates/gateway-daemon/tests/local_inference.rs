use gateway_application::local_inference::{
    LocalInferenceError, LocalInferencePort, LocalInferenceRequest,
};
use gateway_daemon::local_inference::HttpLocalInferenceAdapter;
use serde_json::json;
use std::io::{Read, Write};
use std::net::TcpListener;
use std::thread;

fn request() -> LocalInferenceRequest {
    LocalInferenceRequest {
        schema_version: "1.0".into(),
        role: "semantic-interpreter".into(),
        input_contract: "local-inference/1.0".into(),
        output_contract: "semantic-proposal/1.0".into(),
        prompt: "Read README.md".into(),
        output_schema: json!({"type": "object"}),
    }
}

fn invoke(
    status: u16,
    body: String,
) -> Result<gateway_application::local_inference::LocalInferenceProposal, LocalInferenceError> {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let endpoint = format!("http://{}", listener.local_addr().unwrap());
    let server = thread::spawn(move || {
        let (mut stream, _) = listener.accept().unwrap();
        let mut data = vec![0; 4096];
        let mut used = 0;
        loop {
            used += stream.read(&mut data[used..]).unwrap();
            if let Some(split) = data[..used].windows(4).position(|b| b == b"\r\n\r\n") {
                let headers = String::from_utf8_lossy(&data[..split]);
                let length: usize = headers
                    .lines()
                    .find_map(|line| line.strip_prefix("Content-Length: "))
                    .unwrap()
                    .parse()
                    .unwrap();
                if used >= split + 4 + length {
                    break;
                }
            }
        }
        let text = String::from_utf8_lossy(&data[..used]);
        assert!(text.starts_with("POST /v1/infer HTTP/1.1"));
        assert!(text.contains("semantic-interpreter"));
        write!(stream, "HTTP/1.1 {status} OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}", body.len()).unwrap();
    });
    let result = HttpLocalInferenceAdapter::new(endpoint, 2)
        .unwrap()
        .infer(&request());
    server.join().unwrap();
    result
}

#[test]
fn provider_neutral_proposals_and_failures() {
    let valid = json!({"schema_version":"1.0", "kind":"proposal", "model_id":"replacement",
                      "artifact_digest":format!("sha256:{}", "a".repeat(64)),
                      "proposal":{"action":"read"}, "metrics":{}});
    assert_eq!(
        invoke(200, valid.to_string()).unwrap().model_id,
        "replacement"
    );
    assert_eq!(
        invoke(503, json!({"error":"runtime_unavailable"}).to_string()).unwrap_err(),
        LocalInferenceError::Unavailable
    );
    assert_eq!(
        invoke(200, "bad json".into()).unwrap_err(),
        LocalInferenceError::InvalidProposal
    );
    for (key, value) in [
        ("kind", json!("authoritative")),
        ("schema_version", json!("2.0")),
        ("model_id", json!("")),
        ("artifact_digest", json!("tag")),
    ] {
        let mut bad = valid.clone();
        bad[key] = value;
        assert_eq!(
            invoke(200, bad.to_string()).unwrap_err(),
            LocalInferenceError::InvalidProposal
        );
    }
}

#[test]
fn unavailable_service_and_invalid_requests_are_bounded() {
    assert!(HttpLocalInferenceAdapter::new("file:///tmp/model".into(), 1).is_err());
    assert!(HttpLocalInferenceAdapter::new("https://localhost#fragment".into(), 1).is_err());
    assert!(HttpLocalInferenceAdapter::new("http://localhost".into(), 0).is_err());
    let adapter = HttpLocalInferenceAdapter::new("http://127.0.0.1:1".into(), 1).unwrap();
    assert_eq!(
        adapter.infer(&request()).unwrap_err(),
        LocalInferenceError::Unavailable
    );
    let mut invalid = request();
    invalid.prompt.clear();
    assert_eq!(
        adapter.infer(&invalid).unwrap_err(),
        LocalInferenceError::InvalidRequest
    );
    assert!(HttpLocalInferenceAdapter::from_environment().is_ok());
}
