//! Differential valid-source conformance: independent Python projection vs Rust.
use gateway_cgsl::compile;
use serde_json::{Value, json};
use std::{
    fs,
    io::Write,
    path::PathBuf,
    process::{Command, Stdio},
};

#[test]
fn independent_implementations_agree_on_valid_source_ir_and_digest() {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..");
    let corpus = root.join("tests/fixtures/cgsl-conformance-v1");
    let manifest: Value =
        serde_json::from_slice(&fs::read(corpus.join("manifest.json")).unwrap()).unwrap();
    let mut sources: Vec<String> = manifest["cases"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|c| c["canonical_ir"].is_string())
        .map(|c| fs::read_to_string(corpus.join(c["source"].as_str().unwrap())).unwrap())
        .collect();
    // Independent fixtures plus changes exercising exact numbers, Unicode,
    // canonical sets, omitted defaults, every task kind and nested expressions.
    let full: Value = serde_json::from_str(&sources[1]).unwrap();
    for i in 0..64 {
        let mut source = full.clone();
        source["id"] = json!(format!("independent-{i}"));
        source["task"] = json!(
            [
                "ANALYZE",
                "ANALYZE_PERFORMANCE",
                "INSPECT",
                "CREATE",
                "MODIFY",
                "VERIFY"
            ][i % 6]
        );
        source["goal"]["description"] = json!(format!("é😀 \"\\\n {i}"));
        source["input"] = json!([
            {"role":"z", "value":{"kind":"LITERAL","value":{"kind":"INTEGER","value":i64::MIN + i as i64}}},
            {"role":"a", "value":{"kind":"LITERAL","value":{"kind":"DECIMAL","value":{"unscaled":"000123456789012345678901234567890","scale":18}}}},
            {"role":"set", "value":{"kind":"LITERAL","value":{"kind":"SET","value":[{"kind":"INTEGER","value":9},{"kind":"INTEGER","value":-4}]}}},
            {"role":"boolean", "value":{"kind":"LITERAL","value":{"kind":"BOOLEAN","value":i % 2 == 0}}},
            {"role":"string", "value":{"kind":"LITERAL","value":{"kind":"STRING","value":"é😀\n\"\\"}}},
            {"role":"symbol", "value":{"kind":"LITERAL","value":{"kind":"SYMBOL","value":"pending"}}}
        ]);
        source["assumption"][0]["confidence"] = json!({"kind":"SCORE","value":0.94217});
        source["verification"]["checks"]
            .as_array_mut()
            .unwrap()
            .reverse();
        source["desired_state"]["expression"] = json!({"kind":"ALL","value":[
            {"kind":"NOT","value":{"kind":"CONDITION","value":"p95-target"}},
            {"kind":"CONDITION","value":"p95-target"}
        ]});
        sources.push(source.to_string());
    }
    let mut minimal: Value = serde_json::from_str(&sources[0]).unwrap();
    for key in [
        "input",
        "context",
        "state",
        "desired_state",
        "observation",
        "history",
        "constraint",
        "requires",
        "evidence",
        "assumption",
        "policy",
        "process",
    ] {
        minimal.as_object_mut().unwrap().remove(key);
    }
    sources.push(minimal.to_string());
    let interpreter = std::env::var_os("CG_CONFORMANCE_PYTHON")
        .unwrap_or_else(|| if cfg!(windows) { "python" } else { "python3" }.into());
    let mut child = Command::new(interpreter)
        .arg(root.join("tests/conformance/cgsl_reference.py"))
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("Python conformance witness required");
    let input = serde_json::to_vec(&sources).unwrap();
    // Feed concurrently: output can exceed pipe capacity for the full corpus.
    let mut stdin = child.stdin.take().unwrap();
    let writer = std::thread::spawn(move || {
        stdin.write_all(&input).unwrap();
    });
    let output = child.wait_with_output().unwrap();
    writer.join().unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let results: Vec<Value> = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(sources.len(), results.len());
    for (index, (source, independent)) in sources.iter().zip(results).enumerate() {
        let compiled = compile("independent", source.as_bytes()).unwrap();
        let task = compiled.task_shape().unwrap();
        assert_eq!(
            task.to_canonical_json().unwrap(),
            independent["canonical_ir"].as_str().unwrap(),
            "source {index}"
        );
        assert_eq!(
            task.content_digest().unwrap().as_str(),
            independent["sha256"].as_str().unwrap(),
            "source {index}"
        );
    }
}
