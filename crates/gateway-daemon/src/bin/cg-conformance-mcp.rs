use gateway_daemon::conformance_connector::{Profile, bundle, serve};
use std::io::Write;

fn run() -> std::io::Result<()> {
    let args: Vec<_> = std::env::args().skip(1).collect();
    let invalid = || std::io::Error::from(std::io::ErrorKind::InvalidInput);
    if args.len() != 2 {
        return Err(invalid());
    }
    let profile = Profile::parse(&args[1]).ok_or_else(invalid)?;
    match args[0].as_str() {
        "describe" => Ok(serde_json::to_writer_pretty(
            std::io::stdout().lock(),
            &bundle(profile),
        )?),
        "stdio" => serve(profile, std::io::stdin().lock(), std::io::stdout().lock()),
        _ => Err(invalid()),
    }
}
fn main() {
    if run().is_err() {
        let _ = writeln!(
            std::io::stderr(),
            "cg-conformance-mcp: invalid command or protocol/IO failure"
        );
        std::process::exit(1);
    }
}
