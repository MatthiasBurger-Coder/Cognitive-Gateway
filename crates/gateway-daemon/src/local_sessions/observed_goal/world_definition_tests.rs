//! Genuine native CLI definitions, no allocations/provider calls/authority grants.
use super::*;
use crate::{codex_workspace::LocalWorkspaceResolver, local_sessions::registered_connector_goal};
use gateway_application::codex::{WorkspaceReference, WorkspaceResolver};
use std::{
    path::PathBuf,
    process::Command,
    time::{SystemTime, UNIX_EPOCH},
};

struct Fixture {
    root: PathBuf,
    launch: Vec<String>,
    saved: Vec<(PathBuf, Vec<u8>)>,
}
impl Fixture {
    fn new() -> Self {
        let root = std::env::temp_dir().join(format!(
            "cg408-defs-{}-{}",
            std::process::id(),
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::create_dir_all(&root).unwrap();
        let support = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/support");
        let bins = std::env::var_os("CG_QUALIFICATION_BIN_DIR")
            .map(PathBuf::from)
            .unwrap_or_else(|| {
                std::env::current_exe()
                    .unwrap()
                    .parent()
                    .unwrap()
                    .parent()
                    .unwrap()
                    .to_owned()
            });
        let output=Command::new("python3").arg("-c").arg("import sys,json;from pathlib import Path;sys.path.insert(0,sys.argv[1]);import complete_before_session as f;launch,_,_,_=f.configure(Path(sys.argv[2]));print('WORLD_NATIVE_SETUP='+json.dumps(launch))").arg(support).arg(&root).env("CG_QUALIFICATION_BIN_DIR",bins).output().unwrap();
        assert!(
            output.status.success(),
            "native world setup failed: {}",
            String::from_utf8_lossy(&output.stderr)
        );
        let launch: Vec<String> = serde_json::from_str(
            String::from_utf8(output.stdout)
                .unwrap()
                .lines()
                .find_map(|l| l.strip_prefix("WORLD_NATIVE_SETUP="))
                .expect("actual setup marker"),
        )
        .unwrap();
        let path = PathBuf::from(&launch[13]);
        let bytes = std::fs::read(&path).unwrap();
        let config: Value = serde_json::from_slice(&bytes).unwrap();
        let mut saved = vec![(path, bytes)];
        for doc in config["mappings"][0]["sessions"]["observed_goal_runtime"]["documents"]
            .as_array()
            .unwrap()
        {
            let path = PathBuf::from(doc["path"].as_str().unwrap());
            saved.push((path.clone(), std::fs::read(path).unwrap()));
        }
        Self {
            root,
            launch,
            saved,
        }
    }
    fn restore(&self) {
        for (path, bytes) in &self.saved {
            std::fs::write(path, bytes).unwrap();
        }
    }
    fn capture(&self) -> Result<super::super::definitions::ReadonlyDefinitions, SessionError> {
        let flag = |name: &str| {
            self.launch[self.launch.iter().position(|v| v == name).unwrap() + 1].clone()
        };
        let resolver = LocalWorkspaceResolver::from_json(
            &std::fs::read_to_string(flag("--admission")).unwrap(),
        )
        .map_err(crate::local_sessions::error)?;
        let binding = resolver
            .resolve(&WorkspaceReference {
                working_directory: flag("--cwd"),
                repository: flag("--repository"),
            })
            .map_err(crate::local_sessions::error)?;
        let host = resolver
            .host(&binding)
            .map_err(crate::local_sessions::error)?;
        let config = host.sessions.as_ref().unwrap();
        let goal = registered_connector_goal(config, &binding.canonical_scope)?;
        config
            .observed_goal_runtime
            .as_ref()
            .unwrap()
            .capture(&host, &goal, config.execution)
    }
    fn mutate(&self, case: &str) {
        let output=Command::new("python3").arg("-c").arg("import sys;from pathlib import Path;sys.path.insert(0,sys.argv[1]);import complete_before_session as f;f.mutate_case(Path(sys.argv[2]),sys.argv[3])").arg(PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/support")).arg(&self.root).arg(case).output().unwrap();
        assert!(
            output.status.success(),
            "controlled mutation setup failed: {}",
            String::from_utf8_lossy(&output.stderr)
        );
    }
}
impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.root);
    }
}
#[test]
fn native_world_definitions_bind_complete_universe_independent_of_goal_and_reject_repinned_substitutions()
 {
    if std::env::var_os("CG_COGNITIVE_TEST_DATABASE").is_none() {
        return;
    }
    let f = Fixture::new();
    let actual = f.capture().unwrap();
    let before = actual.before.as_ref().unwrap();
    assert_eq!(before.universe.objects.len(), 3);
    assert_eq!(
        actual
            .task
            .data()
            .desired_state
            .as_ref()
            .unwrap()
            .conditions()
            .len(),
        2
    );
    assert!(
        before
            .normalization
            .fields
            .values()
            .any(|s| s.to_string() == "state.hidden.bytes")
    );
    assert_eq!(actual.normalizations.len(), 3);
    assert!(!f.root.join("wire.jsonl").exists());
    drop(actual);
    for case in [
        "missing-member",
        "duplicate-subject",
        "missing-version",
        "overlap-subject",
        "foreign-source",
        "caller-source",
        "non-atomic",
        "anchor-in-world",
        "invalid-age",
        "foreign-domain",
        "bad-header",
        "unknown-route",
        "mutation-route",
        "epoch-argument",
        "foreign-task",
        "foreign-target",
        "duplicate-member",
        "unassociated-member",
        "unknown-semantics",
        "missing-selector",
        "missing-identity",
        "missing-field",
        "extra-field",
        "weak-source",
        "wrong-credential-profile",
        "wrong-age",
    ] {
        f.mutate(case);
        let outcome = f.capture();
        f.restore();
        assert!(outcome.is_err(), "repinned {case} accepted");
        assert!(
            !f.root.join("wire.jsonl").exists(),
            "definition loading cannot dispatch a provider"
        );
    }
    f.capture().unwrap();
}
