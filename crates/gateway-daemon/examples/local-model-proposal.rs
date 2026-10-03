//! Consume the optional service without embedding a model or bypassing validation.
use gateway_application::local_inference::{LocalInferencePort, LocalInferenceRequest};
use gateway_daemon::local_inference::HttpLocalInferenceAdapter;
use std::io::Read;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let mut input = String::new();
    std::io::stdin()
        .take(2 * 1024 * 1024)
        .read_to_string(&mut input)?;
    let request: LocalInferenceRequest = serde_json::from_str(&input)?;
    let result = HttpLocalInferenceAdapter::from_environment()
        .and_then(|adapter| adapter.infer(&request))
        .map_err(|error| format!("local inference: {error:?}"))?;
    println!("{}", serde_json::to_string(&result)?);
    Ok(())
}
