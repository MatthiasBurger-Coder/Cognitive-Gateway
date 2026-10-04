//! Dedicated private-stdio entrypoint; no environment/auth-store reads.
use gateway_application::codex::{CodexFacade, WorkspaceReference, WorkspaceResolver};
use gateway_daemon::codex_workspace::LocalWorkspaceResolver;
use gateway_daemon::local_mcp::{
    LaunchBinding, MAX_FRAME_BYTES, Server, transport::StdioTransport,
};
use std::io::Read;
use std::time::Duration;

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    if args == ["--help"] {
        eprintln!(
            "cg-mcp --client-name NAME --client-version VERSION --principal ID --workspace ID --project ID --binding ID\nPrivate stdio MCP; trusted launcher arguments required. No provider credentials. Optional admission: --admission FILE --cwd ABSOLUTE_PATH --repository ABSOLUTE_PATH --session ID."
        );
        return;
    }
    let keys = [
        "--client-name",
        "--client-version",
        "--principal",
        "--workspace",
        "--project",
        "--binding",
    ];
    let mut values = Vec::new();
    for key in keys {
        let matches: Vec<_> = args
            .chunks(2)
            .filter(|pair| pair.first().is_some_and(|value| value == key))
            .collect();
        if matches.len() != 1 || matches[0].len() != 2 {
            fail();
        }
        values.push(matches[0][1].as_str());
    }
    if args.len() != 12 && args.len() != 20 {
        fail();
    }
    let Some(binding) = LaunchBinding::new(
        values[0], values[1], values[2], values[3], values[4], values[5],
    ) else {
        fail();
    };
    let mut transport = StdioTransport::new(std::io::stdin(), std::io::stdout(), MAX_FRAME_BYTES);
    let mut server = if args.len() == 20 {
        let mut admission_values = Vec::new();
        for key in ["--admission", "--cwd", "--repository", "--session"] {
            let matches: Vec<_> = args.chunks(2).filter(|pair| pair[0] == key).collect();
            if matches.len() != 1 || matches[0].len() != 2 {
                fail();
            }
            admission_values.push(matches[0][1].as_str());
        }
        let mut text = String::new();
        let file = std::fs::File::open(admission_values[0]).unwrap_or_else(|_| fail());
        file.take(MAX_FRAME_BYTES as u64)
            .read_to_string(&mut text)
            .unwrap_or_else(|_| fail());
        let resolver = LocalWorkspaceResolver::from_json(&text).unwrap_or_else(|_| fail());
        let admitted = resolver
            .resolve(&WorkspaceReference {
                working_directory: admission_values[1].into(),
                repository: admission_values[2].into(),
            })
            .unwrap_or_else(|_| fail());
        if admitted.scope
            != serde_json::json!({"workspace_id":values[3],"project_id":values[4],"binding_id":values[5]})
            || admitted.session.principal != values[2]
            || admitted.session.session_id != admission_values[3]
        {
            fail();
        }
        let host = resolver.host(&admitted).unwrap_or_else(|_| fail());
        let facade = CodexFacade::with_binding(admitted, host).unwrap_or_else(|_| fail());
        Server::with_application(binding, Box::new(facade))
    } else {
        Server::new(binding)
    };
    if server
        .serve(
            &mut transport,
            Duration::from_secs(300),
            Duration::from_secs(2),
        )
        .is_err()
    {
        eprintln!("Local MCP transport terminated.");
        std::process::exit(1);
    }
}
fn fail() -> ! {
    eprintln!("Invalid local MCP launch binding; use cg-mcp --help.");
    std::process::exit(2)
}
