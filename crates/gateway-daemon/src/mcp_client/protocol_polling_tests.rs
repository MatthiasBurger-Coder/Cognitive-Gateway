//! Real stdio invalidation drain with guards slower than the short wait.
use super::*;
use crate::mcp_client::transport::StdioConfig;
use std::{
    path::PathBuf,
    sync::atomic::{AtomicUsize, Ordering},
    time::{SystemTime, UNIX_EPOCH},
};
struct Peer {
    root: PathBuf,
    session: Session,
}
impl Peer {
    fn new(mode: &str) -> Self {
        let root = std::env::temp_dir().join(format!(
            "cg-slow-invalidation-{}-{}",
            std::process::id(),
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::create_dir(&root).unwrap();
        let script = root.join("peer.py");
        std::fs::write(&script,r#"import json,sys,pathlib,time
root=pathlib.Path(sys.argv[1]); mode=sys.argv[2]
def emit(v):
 sys.stdout.write(json.dumps(v)+'\n');sys.stdout.flush()
for line in sys.stdin:
 r=json.loads(line)
 with (root/'wire.jsonl').open('a') as f:f.write(json.dumps(r)+'\n')
 m=r['method']
 if 'id' not in r:continue
 if m=='initialize':v={'protocolVersion':'2025-11-25','serverInfo':{'name':'controlled','version':'1.0.0'},'capabilities':{'tools':{'listChanged':True}}}
 elif m=='tools/list':v={'tools':[]}
 else:v={}
 response={'jsonrpc':'2.0','id':r['id'],'result':v}
 notice={'jsonrpc':'2.0','method':'notifications/tools/list_changed'}
 if m=='tools/list' and mode=='buffered':
  sys.stdout.write(json.dumps(response)+'\n'+json.dumps(notice)+'\n');sys.stdout.flush()
 else:emit(response)
 if m=='tools/list':
  if mode=='queued':
   end=time.monotonic()+3
   while not (root/'release').exists():
    if time.monotonic()>=end:sys.exit(2)
    time.sleep(.001)
   emit(notice)
  (root/'sent').write_text('notification flushed')
"#).unwrap();
        let config = StdioConfig {
            executable: PathBuf::from("/usr/bin/python3"),
            arguments: vec![
                "-I".into(),
                script.to_string_lossy().into_owned(),
                root.to_string_lossy().into_owned(),
                mode.into(),
            ],
            directory: root.clone(),
            environment: Default::default(),
        };
        let mut session = Session::new(Limits::default(), Arc::new(()), "1.0.0".into());
        session.connect(&config, &context()).unwrap();
        session.discover(&context()).unwrap();
        if mode == "queued" {
            // Empty poll first: clear cached Tokio readiness from tools/list.
            assert_eq!(session.refresh(&context()), Ok(()));
        }
        std::fs::write(root.join("release"), b"release").unwrap();
        let until = Instant::now() + Duration::from_secs(2);
        while !root.join("sent").exists() {
            assert!(Instant::now() < until);
            std::thread::sleep(Duration::from_millis(1))
        }
        Self { root, session }
    }
    fn effects(&self) -> usize {
        std::fs::read_to_string(self.root.join("wire.jsonl"))
            .unwrap()
            .lines()
            .filter(|line| serde_json::from_str::<Value>(line).unwrap()["method"] == "tools/call")
            .count()
    }
}
impl Drop for Peer {
    fn drop(&mut self) {
        self.session.close(SessionState::Disconnected, &context());
        let _ = std::fs::remove_dir_all(&self.root);
    }
}
fn context() -> CallContext {
    CallContext::new(
        NonEmptyText::new("slow-drain").unwrap(),
        Instant::now() + Duration::from_secs(2),
    )
}
#[test]
fn slow_live_guard_preserves_buffered_and_os_queued_invalidations_before_effect() {
    for mode in ["buffered", "queued"] {
        let mut peer = Peer::new(mode);
        let checks = Arc::new(AtomicUsize::new(0));
        let count = checks.clone();
        let ctx = context().with_guard(Arc::new(move || {
            count.fetch_add(1, Ordering::SeqCst);
            std::thread::sleep(Duration::from_millis(5));
            Ok(())
        }));
        let deadline = ctx.deadline;
        let result = peer.session.refresh(&ctx);
        // Reproduce the real caller's gate: a falsely benign refresh would allow the
        // next request. This must remain absent from the actual peer's wire log.
        if result.is_ok() {
            let _ = peer.session.rpc("tools/call", json!({}), &ctx);
        }
        assert_eq!(result, Err(Error::InventoryChanged), "{mode}");
        assert!(peer.session.inventory.is_none());
        assert_eq!(peer.effects(), 0, "{mode}");
        assert!(checks.load(Ordering::SeqCst) >= 2);
        assert_eq!(ctx.deadline, deadline);
    }
}
#[test]
fn slow_guard_revocation_cannot_be_mistaken_for_a_benign_poll_timeout() {
    let mut peer = Peer::new("queued");
    let checks = Arc::new(AtomicUsize::new(0));
    let count = checks.clone();
    let ctx = context().with_guard(Arc::new(move || {
        std::thread::sleep(Duration::from_millis(5));
        if count.fetch_add(1, Ordering::SeqCst) >= 1 {
            Err(Error::SessionDenied(ConnectorFailureCode::PermissionDenied))
        } else {
            Ok(())
        }
    }));
    assert_eq!(
        peer.session.refresh(&ctx),
        Err(Error::SessionDenied(ConnectorFailureCode::PermissionDenied))
    );
    assert_eq!(peer.effects(), 0);
}
