//! Dedicated private-stdio entrypoint; environment names only, no auth-store reads.
use gateway_daemon::codex_workspace::admit_local;
use gateway_daemon::local_mcp::{
    LaunchBinding, RuntimeLimits, Server, environment_allowed, transport::StdioTransport,
};
use std::io::Read;
use std::time::Duration;

fn main() {
    std::panic::set_hook(Box::new(|_| {
        eprintln!("CG_INTERNAL_ERROR: local worker failed")
    }));
    let mut args: Vec<String> = std::env::args().skip(1).collect();
    if args == ["--help"] {
        eprintln!(
            "cg-mcp --client-name NAME --client-version VERSION --principal ID --workspace ID --project ID --binding ID\nPrivate stdio MCP; trusted launcher arguments required. No provider credentials. Optional --runtime-limits FILE (bounded JSON); --diagnostics prints default limits. Optional admission: --admission FILE --cwd ABSOLUTE_PATH --repository ABSOLUTE_PATH --session ID."
        );
        return;
    }
    if args == ["--diagnostics"] {
        eprintln!(
            "{}",
            serde_json::json!({"limits":RuntimeLimits::default(),"max_in_flight":1,"queue_capacity":0,"automatic_retry":false})
        );
        return;
    }
    let mut limits = RuntimeLimits::default();
    if let Some(index) = args.iter().position(|arg| arg == "--runtime-limits") {
        if index % 2 != 0 || index + 1 >= args.len() {
            fail();
        }
        let file = std::fs::File::open(&args[index + 1]).unwrap_or_else(|_| fail());
        let mut text = String::new();
        file.take(4097)
            .read_to_string(&mut text)
            .unwrap_or_else(|_| fail());
        if text.len() > 4096 {
            fail();
        }
        limits = serde_json::from_str(&text).unwrap_or_else(|_| fail());
        limits.validate().unwrap_or_else(|_| fail());
        args.drain(index..index + 2);
    }
    if !environment_allowed(std::env::vars_os().map(|(name, _)| name)) {
        eprintln!("Local MCP credential environment denied; launch with a clean environment.");
        std::process::exit(2);
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
    let mut transport = StdioTransport::new(
        std::io::stdin(),
        std::io::stdout(),
        limits.input_bytes.max(limits.output_bytes),
    );
    let mut server = if args.len() == 20 {
        let mut admission_values = Vec::new();
        for key in ["--admission", "--cwd", "--repository", "--session"] {
            let matches: Vec<_> = args.chunks(2).filter(|pair| pair[0] == key).collect();
            if matches.len() != 1 || matches[0].len() != 2 {
                fail();
            }
            admission_values.push(matches[0][1].as_str());
        }
        let (_, facade) = admit_local(
            admission_values[0], admission_values[1], admission_values[2],
            admission_values[3], values[2],
            &serde_json::json!({"workspace_id":values[3],"project_id":values[4],"binding_id":values[5]}),
        ).unwrap_or_else(|_| fail());
        Server::with_application(binding, Box::new(facade))
    } else {
        Server::new(binding)
    };
    let read_timeout = Duration::from_millis(limits.idle_timeout_ms);
    let write_timeout = Duration::from_millis(limits.write_timeout_ms);
    server = server.with_limits(limits).unwrap_or_else(|_| fail());
    if server
        .serve(&mut transport, read_timeout, write_timeout)
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
