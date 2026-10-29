use gateway_daemon::github_connector::{CurlReader, Failure, GitHubConnector};
use std::io::Write;
fn run() -> Result<(), Failure> {
    let args: Vec<_> = std::env::args().skip(1).collect();
    if args.len() < 2 {
        return Err(Failure::Configuration);
    }
    let reader = CurlReader::new(
        if args[0] == "stdio" && args.len() == 3 {
            args[2].clone().into()
        } else {
            "/usr/bin/curl".into()
        },
        std::env::var("GITHUB_PERSONAL_ACCESS_TOKEN").ok(),
    )?;
    let connector = GitHubConnector::new(&args[1], reader)?;
    match (args[0].as_str(), args.len()) {
        ("stdio", 2 | 3) => connector.serve(std::io::stdin().lock(), std::io::stdout().lock()),
        ("describe", 2) => {
            serde_json::to_writer_pretty(std::io::stdout().lock(), &connector.bundle())
                .map_err(|_| Failure::Unavailable)?;
            Ok(())
        }
        ("resolve", 3) => {
            serde_json::to_writer(
                std::io::stdout().lock(),
                &connector.resolve_issue(&args[2])?,
            )
            .map_err(|_| Failure::Unavailable)?;
            Ok(())
        }
        _ => Err(Failure::Configuration),
    }
}
fn main() {
    if run().is_err() {
        let _ = writeln!(
            std::io::stderr(),
            "cg-github-mcp failed; check command, repository, credentials and admitted configuration"
        );
        std::process::exit(1);
    }
}
