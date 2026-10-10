//! Headless operator adapter over the same admitted application facade as MCP.
use gateway_application::codex::{CodexApplicationPort, RequestContext};
use gateway_daemon::{codex_workspace::admit_local, local_mcp};
use serde_json::json;
use std::{
    collections::BTreeMap,
    io::Read,
    time::{Duration, Instant},
};

fn fail(code: &str, action: &str) -> ! {
    eprintln!("{code}: {action}");
    std::process::exit(2)
}

fn main() {
    std::panic::set_hook(Box::new(|_| {
        eprintln!("CG_INTERNAL_ERROR: local worker failed")
    }));
    let mut args: Vec<String> = std::env::args().skip(1).collect();
    if args == ["--help"] {
        println!(
            "cg-local --check | --operation OP --request FILE\nRequired launch options (same as cg-mcp): --client-name NAME --client-version VERSION --principal ID --workspace ID --project ID --binding ID --admission FILE --cwd ABSOLUTE_PATH --repository ABSOLUTE_PATH --session ID\nJSON stdout; fixed diagnostics stderr. Exit 0: success, 1: facade denial, 2: setup/input failure. No retries or provider credentials."
        );
        return;
    }
    if !local_mcp::environment_allowed(std::env::vars_os().map(|(name, _)| name)) {
        fail(
            "CG_CREDENTIAL_ENV_DENIED",
            "launch with an empty environment (env -i)",
        );
    }
    let check = args.first().is_some_and(|arg| arg == "--check");
    if check {
        args.remove(0);
    }
    let mut options = BTreeMap::new();
    if args.len() % 2 != 0 {
        fail(
            "CG_INVALID_INPUT",
            "use cg-local --help; options require values",
        );
    }
    for pair in args.chunks_exact(2) {
        if options.insert(pair[0].as_str(), pair[1].as_str()).is_some() {
            fail("CG_INVALID_INPUT", "use unique launch options");
        }
    }
    let required = [
        "--client-name",
        "--client-version",
        "--principal",
        "--workspace",
        "--project",
        "--binding",
        "--admission",
        "--cwd",
        "--repository",
        "--session",
    ];
    if required.iter().any(|key| !options.contains_key(key))
        || options.keys().any(|key| {
            !required.contains(key) && (check || !["--operation", "--request"].contains(key))
        })
        || (!check && (!options.contains_key("--operation") || !options.contains_key("--request")))
    {
        fail(
            "CG_INVALID_INPUT",
            "supply all launch options; use cg-local --help",
        );
    }
    if local_mcp::LaunchBinding::new(
        options["--client-name"],
        options["--client-version"],
        options["--principal"],
        options["--workspace"],
        options["--project"],
        options["--binding"],
    )
    .is_none()
    {
        fail(
            "CG_INVALID_INPUT",
            "use bounded noncredential identity tokens",
        );
    }
    let scope = json!({"workspace_id":options["--workspace"],"project_id":options["--project"],"binding_id":options["--binding"]});
    let (binding, facade) = admit_local(
        options["--admission"],
        options["--cwd"],
        options["--repository"],
        options["--session"],
        options["--principal"],
        &scope,
    )
    .unwrap_or_else(|error| {
        fail(
            error.code(),
            "check admission schema, absolute roots, unique mapping and matching identity/session",
        )
    });
    if check {
        println!(
            "{}",
            json!({"status":"ready","mcp_protocol_version":local_mcp::PROTOCOL_VERSION,"schema_version":"1.0","admission_schema_version":1,"scope":binding.scope,"canonical_scope":binding.canonical_scope,"principal":binding.session.principal,"session_id":binding.session.session_id,"mapping_revision":binding.mapping_revision,"client_name":options["--client-name"],"client_version":options["--client-version"],"mutations_enabled":false,"limits":local_mcp::RuntimeLimits::default()})
        );
        return;
    }
    let file = std::fs::File::open(options["--request"])
        .unwrap_or_else(|_| fail("CG_INVALID_INPUT", "provide a readable request file"));
    let mut bytes = Vec::new();
    file.take(local_mcp::MAX_FRAME_BYTES as u64 + 1)
        .read_to_end(&mut bytes)
        .unwrap_or_else(|_| fail("CG_INVALID_INPUT", "provide a UTF-8 JSON request"));
    let request = local_mcp::decode_request(&bytes).unwrap_or_else(|| {
        fail(
            "CG_INVALID_INPUT",
            "provide bounded JSON without duplicate keys or credential fields",
        )
    });
    let context = RequestContext::new(
        "local-cli-1".into(),
        Instant::now() + Duration::from_secs(30),
    );
    let response = facade.execute_with_context(options["--operation"], &request, &context);
    let output = response.to_string();
    if output.len() >= local_mcp::MAX_FRAME_BYTES {
        fail("CG_LIMIT_EXCEEDED", "reduce the admitted resource size");
    }
    println!("{output}");
    if response["status"] != "ok" {
        std::process::exit(1);
    }
}
