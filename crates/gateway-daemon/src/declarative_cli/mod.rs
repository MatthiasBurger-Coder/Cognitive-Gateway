//! CG-11 driving adapter. Semantic decisions remain in the application APIs.
mod inputs;
mod json_input;
mod patterns_cli;
mod pipeline;
mod procedure_cli;

use serde::{Serialize, de::DeserializeOwned};
use serde_json::{Value, json};
use std::{
    collections::BTreeMap,
    fmt::Debug,
    io::{self, Read, Write},
};

const HELP: &str = "Cognitive Gateway declarative CLI
Usage:
  cg assess --context <file-or-json> [--json]
  cg plan --intent <file-or-json> --context <file-or-json> [--rules <file-or-json>] [--catalog <directory>] [--json]
  cg resolve --plan <file-or-json> [--catalog <directory>] [--rules <file-or-json>] [--process <file-or-json>] [--policy <file>] [--json]
  cg explain --context <file-or-json> [--intent <file-or-json>] [--rules <file-or-json>] [--catalog <directory>] [--json]
  cg explain --plan <file-or-json> [--catalog <directory>] [--rules <file-or-json>] [--process <file-or-json>] [--policy <file>] [--json]
  cg compile --plan <file-or-json> --policy <file> --projection <file> [--catalog <directory>] [--rules <file-or-json>] [--process <file-or-json>] [--json]
  cg evaluate --procedure <file> --dataset <file> --runtime-version <id> [--json]
  cg simulate --procedure <file> --dataset <file> --runtime-version <id> [--json]
  cg replay --bundle <file> [--json]
  cg patterns --report <file-or-json> [--json]
  cg patterns --scope <project-scope> [--at <unix-seconds>] [--json]

JSON inputs use schema_version 1. A single '-' reads stdin.
Policy and projection files are explicit operator authority, separate from project context.
No command executes a runtime or persists process transitions.
Exit codes: 0 success; 2 usage; 3 input/I/O; 4 assessment; 5 planning;
            6 catalog/resolution; 7 process; 8 policy; 9 compilation;
            10 pattern inspection/database; 11 procedure evaluation failed.
See docs/declarative-cli.md for the input contracts and examples.";

#[derive(Debug)]
struct CliError {
    exit: i32,
    code: &'static str,
    message: String,
}
impl CliError {
    fn new(exit: i32, code: &'static str, message: impl Into<String>) -> Self {
        Self {
            exit,
            code,
            message: message.into(),
        }
    }
    fn json(&self) -> Value {
        json!({"schema_version":1,"error":{"code":self.code,"message":self.message}})
    }
}
// Preserve inner error variant names as diagnostic detail.
fn checked<T, E: Debug>(
    result: Result<T, E>,
    exit: i32,
    code: &'static str,
) -> Result<T, CliError> {
    result.map_err(|e| CliError::new(exit, code, format!("{e:?}")))
}
fn decode<T: DeserializeOwned>(value: Value) -> Result<T, CliError> {
    checked(serde_json::from_value(value), 3, "INVALID_INPUT")
}
fn value<T: Serialize>(input: &T) -> Result<Value, CliError> {
    checked(serde_json::to_value(input), 3, "SERIALIZATION_ERROR")
}
fn version(version: u32) -> Result<(), CliError> {
    if version != 1 {
        return Err(CliError::new(
            3,
            "UNSUPPORTED_VERSION",
            "expected schema_version 1",
        ));
    }
    Ok(())
}

struct Options {
    command: String,
    values: BTreeMap<String, String>,
    json: bool,
}
impl Options {
    fn get(&self, key: &str) -> Option<&str> {
        self.values.get(key).map(String::as_str)
    }
    fn required(&self, key: &str) -> Result<&str, CliError> {
        self.get(key)
            .ok_or_else(|| CliError::new(2, "USAGE", format!("--{key} is required")))
    }
    fn input(&self, key: &str) -> Result<Value, CliError> {
        read_json(self.required(key)?, false)
    }
    fn optional<T: DeserializeOwned + Default>(&self, key: &str) -> Result<T, CliError> {
        self.get(key)
            .map(|s| read_json(s, false).and_then(decode))
            .transpose()
            .map(Option::unwrap_or_default)
    }
}
fn read_json(source: &str, file_only: bool) -> Result<Value, CliError> {
    let text = if !file_only && source == "-" {
        let mut text = String::new();
        checked(io::stdin().read_to_string(&mut text), 3, "INPUT_IO")?;
        text
    } else if !file_only && source.trim_start().starts_with('{') {
        source.to_owned()
    } else {
        checked(std::fs::read_to_string(source), 3, "INPUT_IO")?
    };
    checked(
        serde_json::from_str::<json_input::StrictValue>(&text),
        3,
        "INVALID_JSON",
    )
    .map(|v| v.0)
}
fn parse(arguments: &[String]) -> Result<Options, CliError> {
    let command = arguments
        .get(1)
        .ok_or_else(|| CliError::new(2, "USAGE", "missing command"))?
        .clone();
    let allowed = match command.as_str() {
        "assess" => &["context"][..],
        "patterns" => &["report", "scope", "at"],
        "evaluate" | "simulate" => &["procedure", "dataset", "runtime-version"],
        "replay" => &["bundle"],
        "plan" => &["intent", "context", "rules", "catalog"],
        "resolve" | "compile" => &[
            "plan",
            "rules",
            "catalog",
            "process",
            "policy",
            "projection",
        ],
        "explain" => &[
            "plan", "intent", "context", "rules", "catalog", "process", "policy",
        ],
        _ => {
            return Err(CliError::new(
                2,
                "USAGE",
                format!("unknown command {command:?}"),
            ));
        }
    };
    let mut values = BTreeMap::new();
    let mut json = false;
    let mut args = arguments[2..].iter();
    while let Some(arg) = args.next() {
        if arg == "--json" && !json {
            json = true;
            continue;
        }
        let (key, inline) = arg
            .strip_prefix("--")
            .unwrap_or("")
            .split_once('=')
            .map_or((arg.trim_start_matches("--"), None), |(k, v)| (k, Some(v)));
        if !arg.starts_with("--")
            || !allowed.contains(&key)
            || (key == "projection" && command != "compile")
        {
            return Err(CliError::new(
                2,
                "USAGE",
                format!("unsupported option {arg:?}"),
            ));
        }
        let val = inline
            .or_else(|| args.next().map(String::as_str))
            .filter(|v| !v.is_empty() && !v.starts_with("--"))
            .ok_or_else(|| CliError::new(2, "USAGE", format!("--{key} requires a value")))?;
        if values.insert(key.to_owned(), val.to_owned()).is_some() {
            return Err(CliError::new(2, "USAGE", format!("duplicate --{key}")));
        }
    }
    if values.values().filter(|v| v.as_str() == "-").count() > 1 {
        return Err(CliError::new(2, "USAGE", "stdin may be consumed only once"));
    }
    let options = Options {
        command,
        values,
        json,
    };
    match options.command.as_str() {
        "evaluate" | "simulate" => {
            options.required("procedure")?;
            options.required("dataset")?;
            options.required("runtime-version")?;
        }
        "replay" => {
            options.required("bundle")?;
        }
        "assess" => {
            options.required("context")?;
        }
        "patterns" => {
            if options.get("report").is_some() == options.get("scope").is_some() {
                return Err(CliError::new(
                    2,
                    "USAGE",
                    "provide either --report or --scope",
                ));
            }
            if options.get("at").is_some() && options.get("scope").is_none() {
                return Err(CliError::new(2, "USAGE", "--at requires --scope"));
            }
        }
        "plan" => {
            options.required("context")?;
            options.required("intent")?;
        }
        "compile" => {
            options.required("plan")?;
            options.required("policy")?;
            options.required("projection")?;
        }
        "explain" if options.get("plan").is_none() => {
            options.required("context")?;
            if options.get("policy").is_some() || options.get("process").is_some() {
                return Err(CliError::new(
                    2,
                    "USAGE",
                    "--policy and --process require --plan",
                ));
            }
        }
        _ => {
            options.required("plan")?;
        }
    }
    if options.get("plan").is_some()
        && (options.get("context").is_some() || options.get("intent").is_some())
    {
        return Err(CliError::new(
            2,
            "USAGE",
            "--plan cannot be combined with --context or --intent",
        ));
    }
    Ok(options)
}

/// Runs the product CLI. Arguments include the executable name.
pub fn run<I, S>(arguments: I) -> i32
where
    I: IntoIterator<Item = S>,
    S: Into<String>,
{
    let arguments: Vec<String> = arguments.into_iter().map(Into::into).collect();
    let json = arguments.iter().any(|s| s == "--json");
    let result =
        if arguments.len() == 1 || arguments.iter().skip(1).any(|s| s == "--help" || s == "-h") {
            Ok((HELP.to_owned(), 0))
        } else if arguments.len() == 2 && matches!(arguments[1].as_str(), "--version" | "-V") {
            Ok((format!("cg {}", env!("CARGO_PKG_VERSION")), 0))
        } else {
            parse(&arguments).and_then(|o| {
                let (output, exit) = pipeline::execute(&o)?;
                let text = if o.json {
                    output.to_string()
                } else {
                    render(&o.command, &output)
                };
                Ok((text, exit))
            })
        };
    let (text, exit, stderr) = match result {
        Ok((text, exit)) => (text, exit, false),
        Err(e) => (
            if json {
                e.json().to_string()
            } else {
                format!("{}: {}", e.code, e.message)
            },
            e.exit,
            !json,
        ),
    };
    let result = if stderr {
        writeln!(io::stderr().lock(), "{text}")
    } else {
        writeln!(io::stdout().lock(), "{text}")
    };
    if result.is_err() { 3 } else { exit }
}
fn render(command: &str, output: &Value) -> String {
    let mut text = format!("Cognitive Gateway: {command}\n");
    render_value(output, 0, &mut text);
    text
}
fn render_value(value: &Value, depth: usize, text: &mut String) {
    let pad = "  ".repeat(depth);
    match value {
        Value::Object(fields) => {
            for (name, value) in fields {
                text.push_str(&format!("{pad}{}:\n", name.replace('_', " ")));
                render_value(value, depth + 1, text);
            }
        }
        Value::Array(items) => {
            for value in items {
                text.push_str(&format!("{pad}-\n"));
                render_value(value, depth + 1, text);
            }
            if items.is_empty() {
                text.push_str(&format!("{pad}(empty)\n"));
            }
        }
        Value::String(value) => {
            for line in value.lines() {
                text.push_str(&format!("{pad}{line}\n"));
            }
        }
        value => text.push_str(&format!("{pad}{value}\n")),
    }
}
