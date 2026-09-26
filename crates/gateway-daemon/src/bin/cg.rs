#![forbid(unsafe_code)]

fn main() {
    std::process::exit(gateway_daemon::declarative_cli::run(std::env::args()));
}
