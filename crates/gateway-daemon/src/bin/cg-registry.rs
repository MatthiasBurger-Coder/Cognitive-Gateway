#![forbid(unsafe_code)]

fn main() {
    std::process::exit(gateway_daemon::registry_cli::run(std::env::args()));
}
