//! Dedicated private-stdio entrypoint; no environment/auth-store reads.
use gateway_daemon::local_mcp::{
    LaunchBinding, MAX_FRAME_BYTES, Server, transport::StdioTransport,
};
use std::time::Duration;

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    if args == ["--help"] {
        eprintln!(
            "cg-mcp --client-name NAME --client-version VERSION --principal ID --workspace ID --project ID --binding ID\nPrivate stdio MCP; trusted launcher arguments required. No provider credentials. Admitted application host wiring pending."
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
    if args.len() != 12 {
        fail();
    }
    let Some(binding) = LaunchBinding::new(
        values[0], values[1], values[2], values[3], values[4], values[5],
    ) else {
        fail();
    };
    let mut transport = StdioTransport::new(std::io::stdin(), std::io::stdout(), MAX_FRAME_BYTES);
    if Server::new(binding)
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
