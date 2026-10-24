//! Operator entrypoint for the EPIC-06 capability host, without task lifecycle.
use gateway_daemon::invocation_host::InvocationHost;
use serde_json::json;
use std::{collections::BTreeMap, path::Path};
fn main() {
    let args: Vec<_> = std::env::args().skip(1).collect();
    if args == ["--help"] {
        println!(
            "cg-invoke --admission FILE --principal ID --invocation ID --operation invoke|replay|inspect|cancel"
        );
        return;
    }
    let result = (|| {
        if args.len() != 8 {
            return Err("CG_INVALID_INPUT");
        }
        let mut options = BTreeMap::new();
        for pair in args.as_chunks::<2>().0 {
            if options.insert(pair[0].as_str(), pair[1].as_str()).is_some() {
                return Err("CG_INVALID_INPUT");
            }
        }
        let mut get = |key| options.remove(key).ok_or("CG_INVALID_INPUT");
        let admission = get("--admission")?;
        let principal = get("--principal")?;
        let invocation = get("--invocation")?;
        let operation = get("--operation")?;
        InvocationHost::load(Path::new(admission), principal, invocation)?.execute(operation)
    })();
    match result {
        Ok(value) => {
            println!("{value}");
            if value["status"] == "REJECTED"
                || value["status"] == "FAILED"
                || !value["diagnostic"].is_null()
            {
                std::process::exit(1);
            }
        }
        Err(code) => {
            println!("{}", json!({"status":"FAILED","diagnostic":code}));
            std::process::exit(1);
        }
    }
}
