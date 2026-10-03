use serde_json::Value;
use std::{fs, process::Command};
fn command(action: &str) -> Command {
    let mut command = Command::new(env!("CARGO_BIN_EXE_cg"));
    command.arg(action).arg("--json");
    command
}
#[test]
fn simulate_evaluate_replay_and_tamper_rejection() {
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../tests/fixtures/procedure-evaluation-v1");
    let args = |command: &mut Command| {
        command
            .arg("--procedure")
            .arg(root.join("procedure.json"))
            .arg("--dataset")
            .arg(root.join("historical.json"))
            .arg("--runtime-version")
            .arg("runtime-1");
    };
    let mut simulate = command("simulate");
    args(&mut simulate);
    let output = simulate.output().unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stdout)
    );
    let bundle: Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(bundle["report"]["passed"], true);
    let mut again = command("simulate");
    args(&mut again);
    assert_eq!(output.stdout, again.output().unwrap().stdout);
    let mut evaluate = command("evaluate");
    args(&mut evaluate);
    let incomplete = evaluate.output().unwrap();
    assert_eq!(incomplete.status.code(), Some(11));
    assert_eq!(
        serde_json::from_slice::<Value>(&incomplete.stdout).unwrap()["report"]["passed"],
        false
    );
    let path = std::env::temp_dir().join(format!("cg23-bundle-{}.json", std::process::id()));
    fs::write(&path, &output.stdout).unwrap();
    let replay = command("replay")
        .arg("--bundle")
        .arg(&path)
        .output()
        .unwrap();
    assert!(replay.status.success());
    assert_eq!(replay.stdout, output.stdout);
    let mut altered = bundle;
    altered["report"]["manifest"]["procedure_version"] = 2.into();
    fs::write(&path, altered.to_string()).unwrap();
    let rejected = command("replay")
        .arg("--bundle")
        .arg(&path)
        .output()
        .unwrap();
    fs::remove_file(&path).unwrap();
    assert_eq!(rejected.status.code(), Some(3));
    assert!(String::from_utf8_lossy(&rejected.stdout).contains("INVALID_EVALUATION_BUNDLE"));
    assert_eq!(command("simulate").output().unwrap().status.code(), Some(2));
}

#[test]
fn malformed_contracts_and_invalid_simulation_baselines_are_reported() {
    use gateway_domain::{
        ReferenceId,
        procedure_evaluation::{EvaluationDataset, ReplayCase},
    };
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../tests/fixtures/procedure-evaluation-v1");
    let procedure = fs::read_to_string(root.join("procedure.json")).unwrap();
    let historical = fs::read_to_string(root.join("historical.json")).unwrap();
    let check = |action: &str, p: &str, d: &str, runtime: &str, code: &str| {
        let output = command(action)
            .args([
                "--procedure",
                p,
                "--dataset",
                d,
                "--runtime-version",
                runtime,
            ])
            .output()
            .unwrap();
        assert_eq!(output.status.code(), Some(3));
        assert!(
            String::from_utf8_lossy(&output.stdout).contains(code),
            "{}",
            String::from_utf8_lossy(&output.stdout)
        );
    };
    check(
        "evaluate",
        "{}",
        &historical,
        "runtime-1",
        "INVALID_PROCEDURE",
    );
    check("evaluate", &procedure, "{}", "runtime-1", "INVALID_INPUT");
    check(
        "evaluate",
        &procedure,
        &historical,
        " ",
        "INVALID_RUNTIME_VERSION",
    );
    let mut dataset: EvaluationDataset = serde_json::from_str(&historical).unwrap();
    dataset.schema_version = 2;
    check(
        "evaluate",
        &procedure,
        &serde_json::to_string(&dataset).unwrap(),
        "runtime-1",
        "INVALID_DATASET",
    );
    dataset.schema_version = 1;
    let positive = &mut dataset.cases[0];
    positive.snapshot.evidence.clear();
    *positive = ReplayCase::new(
        positive.id.clone(),
        positive.kind,
        positive.expected,
        positive.snapshot.clone(),
    );
    check(
        "simulate",
        &procedure,
        &serde_json::to_string(&dataset).unwrap(),
        "runtime-1",
        "INVALID_BASELINE",
    );
    let mut dataset: EvaluationDataset = serde_json::from_str(&historical).unwrap();
    let mut duplicate = dataset.cases[1].clone();
    duplicate.id = ReferenceId::new("positive.near-0").unwrap();
    dataset.cases.push(duplicate);
    check(
        "simulate",
        &procedure,
        &serde_json::to_string(&dataset).unwrap(),
        "runtime-1",
        "INVALID_EVALUATION",
    );
}
