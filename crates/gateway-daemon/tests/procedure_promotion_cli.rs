#[path = "../../../tests/support/procedure_promotion.rs"]
mod support;
use gateway_application::procedure_promotion::{PromotionError, PromotionStore};
use gateway_daemon::procedure_promotion_store::FilePromotionStore;
use gateway_domain::procedure_promotion::*;
use serde_json::Value;
use std::{
    fs,
    io::Write,
    process::{Command, Stdio},
};
use support::*;

fn command(input: &str, json: bool) -> std::process::Output {
    let mut cmd = Command::new(env!("CARGO_BIN_EXE_cg"));
    cmd.args(["procedures", "--registry", input]);
    if json {
        cmd.arg("--json");
    }
    cmd.output().unwrap()
}
#[test]
fn durable_lifecycle_and_cli_inspection_need_no_model() {
    let root = std::env::temp_dir().join(format!("cg24-durable-{}", std::process::id()));
    fs::create_dir_all(&root).unwrap();
    let path = root.join("registry.json");
    let mut h = History::default();
    let v1 = h.discover(1);
    let v2 = h.discover(2);
    h.canary(&v1, 20);
    h.success(&v1, 20, "pilot1");
    h.push(
        20,
        PromotionCommand::Activate {
            procedure: v1.clone(),
        },
    );
    h.canary(&v2, 20);
    h.success(&v2, 20, "pilot2");
    h.push(
        20,
        PromotionCommand::Supersede {
            procedure: v2.clone(),
            previous: v1.clone(),
        },
    );
    h.push(
        21,
        PromotionCommand::Rollback {
            procedure: v2.clone(),
            restore: Some(v1.clone()),
        },
    );
    let mut store = FilePromotionStore::open(&path).unwrap();
    for (revision, event) in h.journal.events.iter().enumerate() {
        store.append(revision, event.clone()).unwrap();
    }
    drop(store);
    let reopened = FilePromotionStore::open(&path).unwrap();
    assert_eq!(reopened.load().unwrap(), h.journal);
    let output = command(path.to_str().unwrap(), true);
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stdout)
    );
    let report: Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(report["entries"][0]["state"], "ACTIVE");
    assert_eq!(report["entries"][1]["state"], "ROLLED_BACK");
    assert_eq!(report["entries"][1]["predecessor"]["version"], 1);
    assert_eq!(
        report["entries"][0]["executions"][0]["request"]["mode"],
        "CANARY"
    );
    assert_eq!(report["journal"], serde_json::to_value(&h.journal).unwrap());
    assert_eq!(output.stdout, command(path.to_str().unwrap(), true).stdout);
    let human = command(path.to_str().unwrap(), false);
    assert!(human.status.success());
    assert!(String::from_utf8_lossy(&human.stdout).contains("ROLLED_BACK"));
    let mut stdin = Command::new(env!("CARGO_BIN_EXE_cg"))
        .args(["procedures", "--registry", "-", "--json"])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .spawn()
        .unwrap();
    stdin
        .stdin
        .take()
        .unwrap()
        .write_all(&serde_json::to_vec(&h.journal).unwrap())
        .unwrap();
    assert_eq!(stdin.wait_with_output().unwrap().stdout, output.stdout);
    let mut tampered = h.journal;
    tampered.events[3].metadata.at = -1;
    assert_eq!(
        command(&serde_json::to_string(&tampered).unwrap(), true)
            .status
            .code(),
        Some(3)
    );
    assert_eq!(
        command("{\"schema_version\":2,\"events\":[]}", true)
            .status
            .code(),
        Some(3)
    );
    assert_eq!(command("{}", true).status.code(), Some(3));
    assert_eq!(
        Command::new(env!("CARGO_BIN_EXE_cg"))
            .args(["procedures", "--json"])
            .output()
            .unwrap()
            .status
            .code(),
        Some(2)
    );
    fs::remove_dir_all(root).unwrap();
}
#[test]
fn durable_compare_and_append_lock_and_invalid_storage_fail_closed() {
    let root = std::env::temp_dir().join(format!("cg24-file-errors-{}", std::process::id()));
    fs::create_dir_all(&root).unwrap();
    let path = root.join("registry.json");
    let mut store = FilePromotionStore::open(&path).unwrap();
    let mut second = FilePromotionStore::open(&path).unwrap();
    let event = event(
        0,
        10,
        PromotionCommand::Discover {
            procedure: Box::new(procedure(1)),
            discovery_evidence: id("source"),
        },
    );
    store.append(0, event.clone()).unwrap();
    assert_eq!(
        second.append(0, event.clone()),
        Err(PromotionError::Conflict)
    );
    assert!(second.append(1, event.clone()).is_err());
    assert_eq!(store.load().unwrap().events.len(), 1);
    let lock = root.join("registry.json.lock");
    fs::write(&lock, "held").unwrap();
    assert_eq!(
        store.append(1, event.clone()),
        Err(PromotionError::Conflict)
    );
    fs::remove_file(lock).unwrap();
    fs::write(&path, "invalid json").unwrap();
    assert!(FilePromotionStore::open(&path).is_err());
    assert!(store.append(1, event.clone()).is_err());
    fs::write(&path, "{\"schema_version\":2,\"events\":[]}").unwrap();
    assert!(FilePromotionStore::open(&path).is_err());
    fs::remove_file(&path).unwrap();
    let next = root.join("registry.json.next");
    fs::write(&next, "incomplete").unwrap();
    assert!(store.append(0, event.clone()).is_err());
    assert!(store.load().unwrap().events.is_empty());
    fs::remove_file(next).unwrap();
    fs::create_dir(&path).unwrap();
    assert!(FilePromotionStore::open(&path).is_err());
    let mut missing_parent = FilePromotionStore::open(root.join("missing/journal")).unwrap();
    assert!(missing_parent.append(0, event.clone()).is_err());
    fs::remove_dir(&path).unwrap();
    // A failed commit cleans its lock and temporary document.
    fs::create_dir(&path).unwrap();
    assert!(store.append(0, event).is_err());
    assert!(!root.join("registry.json.lock").exists());
    fs::remove_dir_all(root).unwrap();
}
