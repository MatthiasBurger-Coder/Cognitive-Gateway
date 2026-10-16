use gateway_cgsl::*;
use serde_json::{Value as Json, json};

const MINIMAL: &str = include_str!("../../../examples/cgsl-v1/minimal.cgsl");
const INCOMPLETE: &str = include_str!("../../../examples/cgsl-v1/incomplete.cgsl");
fn compile_json(value: &Json) -> Result<CompiledSource, Vec<Diagnostic>> {
    compile("test", value.to_string().as_bytes())
}
fn minimal() -> Json {
    serde_json::from_str(MINIMAL).unwrap()
}
fn incomplete() -> Json {
    serde_json::from_str(INCOMPLETE).unwrap()
}
fn codes(value: &Json) -> Vec<&'static str> {
    compile_json(value)
        .unwrap_err()
        .iter()
        .map(|e| e.code)
        .collect()
}

#[test]
fn frozen_examples_match_existing_canonical_ir() {
    for (source, canonical) in [
        (
            MINIMAL,
            include_str!("../../../tests/fixtures/semantic-task-v1/minimal.canonical.json"),
        ),
        (
            include_str!("../../../examples/cgsl-v1/performance-analysis.cgsl"),
            include_str!(
                "../../../tests/fixtures/semantic-task-v1/performance-analysis.canonical.json"
            ),
        ),
    ] {
        let compiled = compile("frozen", source.as_bytes()).unwrap();
        assert_eq!(
            compiled.task_shape().unwrap().to_canonical_json().unwrap(),
            canonical.trim()
        );
        assert_eq!(compiled, compile("frozen", source.as_bytes()).unwrap());
        for operand in compiled.operands.values() {
            assert!(!serde_json::to_string(operand).unwrap().is_empty());
        }
    }
}

#[test]
fn specification_rejections() {
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../examples/cgsl-v1");
    let expectations: Json =
        serde_json::from_slice(&std::fs::read(root.join("expectations.json")).unwrap()).unwrap();
    for case in expectations["rejected"].as_array().unwrap() {
        let source = std::fs::read(root.join(case["file"].as_str().unwrap())).unwrap();
        let errors = compile("example", &source).unwrap_err();
        assert_eq!(errors[0].code, case["code"], "{case}");
        assert_eq!(errors[0].phase, case["phase"]);
        assert_eq!(errors, compile("example", &source).unwrap_err());
        assert_eq!(
            serde_json::to_value(&errors[0]).unwrap()["severity"],
            "ERROR"
        );
    }
}

#[test]
fn sidecars_preserve_unresolved_meaning_and_block_projection() {
    let result = compile("incomplete", INCOMPLETE.as_bytes()).unwrap();
    assert!(result.task_shape().is_none());
    assert_eq!(
        result.knowledge_gaps[0].diagnostic_code(),
        "SEM_AMBIGUOUS_ENTITY"
    );
    assert_eq!(result.ambiguities[0].candidates.candidates().len(), 2);
    assert_eq!(result.confidence.len(), 1);
    for subject in [
        json!({"kind":"PATH","value":"target"}),
        json!({"kind":"TARGET","value":minimal()["target"]}),
    ] {
        let mut value = incomplete();
        value["knowledge_gap"][0]["subject"] = subject;
        assert!(compile_json(&value).unwrap().task_shape().is_none());
    }
    let mut value = minimal();
    value["confidence"] = incomplete()["confidence"].clone();
    assert!(compile_json(&value).unwrap().task_shape().is_none());
}

#[test]
fn defaults_and_object_order_are_semantically_inert() {
    let mut value = minimal();
    for field in [
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
        "knowledge_gap",
        "ambiguity",
        "confidence",
    ] {
        value.as_object_mut().unwrap().remove(field);
    }
    let expected = compile("original", MINIMAL.as_bytes()).unwrap();
    assert_eq!(
        compile_json(&value).unwrap().task_shape(),
        expected.task_shape()
    );
    let pairs: Vec<_> = value
        .as_object()
        .unwrap()
        .iter()
        .rev()
        .map(|(k, v)| format!("{}:{}", serde_json::to_string(k).unwrap(), v))
        .collect();
    assert_eq!(
        compile("reversed", format!("{{{}}}", pairs.join(",")).as_bytes())
            .unwrap()
            .task_shape(),
        expected.task_shape()
    );
}

#[test]
fn typed_integer_admission_preserves_precision_and_rejects_overflow() {
    let mut value = minimal();
    value["input"] = json!([{"role":"exact", "value":{"kind":"LITERAL", "value":{"kind":"INTEGER", "value":9223372036854775807_i64}}}]);
    let source = value.to_string();
    let compiled = compile("exact", source.as_bytes()).unwrap();
    assert!(
        compiled
            .task_shape()
            .unwrap()
            .to_canonical_json()
            .unwrap()
            .contains("9223372036854775807")
    );
    let overflow = source.replace("9223372036854775807", "9223372036854775808");
    assert!(parse("overflow", overflow.as_bytes()).is_ok());
    assert_eq!(
        compile("overflow", overflow.as_bytes()).unwrap_err()[0].code,
        "SEM_INVALID_OPERAND"
    );
    let fractional = source.replace("9223372036854775807", "1.0");
    assert_eq!(
        compile("fraction", fractional.as_bytes()).unwrap_err()[0].code,
        "SEM_INVALID_OPERAND"
    );
}

#[test]
fn versions_precede_operands_and_do_not_overflow() {
    for version in [
        json!(null),
        json!(1),
        json!("01.0"),
        json!("1"),
        json!("1.0.0"),
        json!("-1.0"),
        json!(".0"),
        json!("1.a"),
    ] {
        let mut value = minimal();
        value["cgsl"] = version;
        value["task"] = json!("BAD");
        assert_eq!(codes(&value), ["VER_INVALID_VERSION"]);
    }
    for version in ["1.1", "2.0", "999999999999999999999999999999.0"] {
        let mut value = minimal();
        value["cgsl"] = json!(version);
        assert_eq!(codes(&value), ["VER_UNSUPPORTED_VERSION"]);
    }
    let errors = compile("v", b"{}").unwrap_err();
    assert_eq!(errors[0].location.span, Span { start: 1, end: 1 });
}

#[test]
fn semantic_invalidity_is_not_a_syntax_failure() {
    for (field, bad) in [
        ("task", json!("bad")),
        ("id", json!("")),
        ("goal", json!({"outcome":"x","description":" "})),
        ("input", json!(null)),
        ("knowledge_gap", json!({})),
        ("ambiguity", json!(null)),
        ("confidence", json!(false)),
    ] {
        let mut value = minimal();
        value[field] = bad;
        assert!(codes(&value).contains(&"SEM_INVALID_OPERAND"), "{field}");
    }
    let mut value = minimal();
    value.as_object_mut().unwrap().remove("id");
    assert!(codes(&value).contains(&"SEM_MISSING_REQUIRED_FIELD"));
    value = minimal();
    value["target"]["reference"]["digest"] = json!("abc");
    assert_eq!(codes(&value), ["SEM_INVALID_OPERAND"]);
    value = minimal();
    value["output"]["schema"]["scope"] = json!("foreign");
    assert_eq!(codes(&value), ["SEM_INVALID_OPERAND"]);
    value = minimal();
    value["verification"]["checks"] = json!([]);
    assert_eq!(codes(&value), ["SEM_INVALID_OPERAND"]);
    value = minimal();
    value["verification"]["checks"] = json!(["OUTPUT_SCHEMA_VALID", "OUTPUT_SCHEMA_VALID"]);
    assert_eq!(codes(&value), ["SEM_DUPLICATE_IDENTITY"]);
    value = minimal();
    value["context"] = json!([value["target"]["reference"], value["target"]["reference"]]);
    let errors = compile_json(&value).unwrap_err();
    assert_eq!(errors[0].code, "SEM_DUPLICATE_IDENTITY");
    assert_eq!(errors[0].related.len(), 1);
}

#[test]
fn invalid_sidecars_fail_without_downstream_guessing() {
    let mut duplicate_diagnostics = incomplete();
    duplicate_diagnostics["goal"]["description"] = json!("");
    let errors = compile_json(&duplicate_diagnostics).unwrap_err();
    assert_eq!(
        errors
            .iter()
            .filter(|e| e.location.path == vec![PathPart::Member("goal".into())])
            .count(),
        1
    );
    for field in ["knowledge_gap", "ambiguity", "confidence"] {
        for bad in [json!(1), json!({"unknown":1}), json!({})] {
            let mut value = incomplete();
            value[field][0] = bad;
            assert!(compile_json(&value).is_err());
        }
        let mut value = incomplete();
        let first = value[field][0].clone();
        value[field].as_array_mut().unwrap().push(first);
        assert!(codes(&value).contains(&"SEM_DUPLICATE_IDENTITY"));
    }
    for (pointer, bad, expected) in [
        (
            "/knowledge_gap/0/subject/kind",
            json!("BAD"),
            "SEM_INVALID_OPERAND",
        ),
        (
            "/knowledge_gap/0/missing_field",
            json!("bad..path"),
            "SEM_INVALID_OPERAND",
        ),
        (
            "/knowledge_gap/0/provenance",
            json!([]),
            "SEM_INVALID_ATTACHMENT",
        ),
        (
            "/ambiguity/0/field",
            json!("workflow"),
            "SEM_INVALID_ATTACHMENT",
        ),
        (
            "/ambiguity/0/candidates",
            json!([]),
            "SEM_INVALID_ATTACHMENT",
        ),
        (
            "/ambiguity/0/candidates",
            json!(true),
            "SEM_INVALID_OPERAND",
        ),
        (
            "/ambiguity/0/candidates/0/provenance",
            json!([]),
            "SEM_INVALID_ATTACHMENT",
        ),
        (
            "/ambiguity/0/candidates/0/value",
            json!("shorthand"),
            "SEM_INVALID_OPERAND",
        ),
        (
            "/confidence/0/field",
            json!("state"),
            "SEM_INVALID_ATTACHMENT",
        ),
        (
            "/confidence/0/value",
            json!({"kind":"BAD"}),
            "SEM_INVALID_OPERAND",
        ),
        (
            "/confidence/0/basis/digest",
            json!("ABC"),
            "SEM_INVALID_OPERAND",
        ),
    ] {
        let mut value = incomplete();
        *value.pointer_mut(pointer).unwrap() = bad;
        assert!(codes(&value).contains(&expected), "{pointer}");
    }
    let mut value = incomplete();
    value["target"] = minimal()["target"].clone();
    assert!(codes(&value).contains(&"SEM_CONFLICTING_CONTEXT"));
    value = incomplete();
    value["ambiguity"][0]["candidates"][1] = value["ambiguity"][0]["candidates"][0].clone();
    assert!(codes(&value).contains(&"SEM_INVALID_ATTACHMENT"));
}

#[test]
fn lexical_matrix_and_exact_numbers() {
    for token in [
        "0",
        "-0",
        "1",
        "-12",
        "1.00",
        "1e0",
        "1E+999999",
        "1e-999999",
        "123456789012345678901234567890",
        "true",
        "false",
        "null",
        "[]",
        "{}",
        r#""\uD83D\uDE00""#,
        r#""é//\n\t\b\f\r\/\\\"""#,
    ] {
        let source = format!("{{\"x\":{token}}}");
        let document = parse("tokens", source.as_bytes()).unwrap();
        let rendered = document.root.json();
        assert_eq!(
            parse("tokens", rendered.as_bytes()).unwrap().root.json(),
            rendered
        );
        if matches!(document.root.member("x").unwrap().value, Value::Number(_)) {
            assert_eq!(document.root.member("x").unwrap().json(), token);
        }
    }
    for token in [
        "01",
        "-",
        "+1",
        "1.",
        "1e",
        "1e+",
        "NaN",
        "Infinity",
        "truex",
        "fals",
        "nul",
        r#""\uD800""#,
        r#""\uDC00""#,
        r#""\q""#,
        "\"raw\nline\"",
    ] {
        let error = parse("tokens", format!("{{\"x\":{token}}}").as_bytes()).unwrap_err();
        assert_eq!(error.code, "SYN_INVALID_TOKEN", "{token}");
    }
    for source in [
        "",
        "[]",
        "{",
        "{\"a\"",
        "{\"a\":",
        "{\"a\":}",
        "{\"a\":,}",
        "{\"a\":[]",
        "{\"a\":[1,]}",
        "{\"a\":\"unterminated",
        "{\"a\":\"backslash\\",
    ] {
        assert_eq!(
            parse("s", source.as_bytes()).unwrap_err().code,
            "SYN_EXPECTED_TOKEN",
            "{source}"
        );
    }
    assert_eq!(
        parse("s", b"{} {}").unwrap_err().code,
        "SYN_TRAILING_CONTENT"
    );
    assert_eq!(
        parse("s", b"{#comment\n}").unwrap_err().code,
        "SYN_INVALID_TOKEN"
    );
}

#[test]
fn byte_spans_decoded_paths_and_duplicates() {
    let source = "{\r\n\t\"é\": [\"😀\", {\"a\": 1, \"\\u0061\": 2}]}";
    let error = parse("unicode", source.as_bytes()).unwrap_err();
    assert_eq!(error.code, "SYN_DUPLICATE_MEMBER");
    assert_eq!(
        &source[error.location.span.start..error.location.span.end],
        "\"\\u0061\""
    );
    assert_eq!(
        &source[error.related[0].span.start..error.related[0].span.end],
        "\"a\""
    );
    assert_eq!(
        error.location.path,
        vec![
            PathPart::Member("é".into()),
            PathPart::Index(1),
            PathPart::Member("a".into())
        ]
    );
    let document = parse("unicode", "{\"é\":[\"😀\"]}".as_bytes()).unwrap();
    let path = vec![PathPart::Member("é".into()), PathPart::Index(0)];
    let location = source_location(&document, &path).unwrap();
    assert_eq!(location.span.end - location.span.start, 6);
    assert!(source_location(&document, &[PathPart::Member("absent".into())]).is_none());
    assert!(source_location(&document, &[PathPart::Index(0)]).is_none());
    assert!(document.root.member("é").unwrap().member("x").is_none());
    assert_eq!(
        parse("eof", b"{\"a\":").unwrap_err().location.span,
        Span { start: 5, end: 5 }
    );
}

#[test]
fn encoding_and_resource_limits_are_explicit() {
    for bytes in [
        vec![255],
        vec![b'{', 0xe2, 0x82],
        "\u{feff}{}".as_bytes().to_vec(),
    ] {
        assert_eq!(
            parse("bad", &bytes).unwrap_err().code,
            "SYN_INVALID_ENCODING"
        );
    }
    assert_eq!(
        parse("limit", &vec![b' '; MAX_SOURCE_BYTES + 1])
            .unwrap_err()
            .code,
        "CGSL_RESOURCE_LIMIT"
    );
    let deep = format!(
        "{{\"x\":{}0{}}}",
        "[".repeat(MAX_DEPTH + 1),
        "]".repeat(MAX_DEPTH + 1)
    );
    assert_eq!(
        parse("limit", deep.as_bytes()).unwrap_err().code,
        "CGSL_RESOURCE_LIMIT"
    );
    let wide = format!("{{\"x\":[{}]}}", vec!["0"; MAX_NODES].join(","));
    assert_eq!(
        parse("limit", wide.as_bytes()).unwrap_err().code,
        "CGSL_RESOURCE_LIMIT"
    );
}

/// Deterministic generated properties; seed and corpus are fixed and require no network.
#[test]
fn generated_roundtrips_and_arbitrary_bytes_never_panic() {
    let mut seed = 186_u64;
    for iteration in 0..2000 {
        seed = seed.wrapping_mul(6364136223846793005).wrapping_add(1);
        let value = json!({"text":format!("é😀\n\\\"{seed}"), "array":[seed,iteration % 2 == 0,null,{"nested":iteration}]});
        let source = value.to_string();
        let ast = parse("property", source.as_bytes()).unwrap();
        assert_eq!(
            serde_json::from_str::<Json>(&ast.root.json()).unwrap(),
            value
        );
        assert_eq!(
            parse("property", ast.root.json().as_bytes())
                .unwrap()
                .root
                .json(),
            ast.root.json()
        );
        let bytes: Vec<_> = (0..(iteration % 80))
            .map(|offset| (seed.rotate_left(offset as u32) & 255) as u8)
            .collect();
        assert_eq!(parse("arbitrary", &bytes), parse("arbitrary", &bytes));
    }
    // Every prefix of a real source is safely rejected or parsed deterministically.
    for end in 0..MINIMAL.len() {
        let _ = compile("prefix", &MINIMAL.as_bytes()[..end]);
    }
}
