#![forbid(unsafe_code)]

fn main() -> std::process::ExitCode {
    let code = gateway_daemon::declarative_cli::run(std::env::args());
    std::process::ExitCode::from(code as u8)
}
