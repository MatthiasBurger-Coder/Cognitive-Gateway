//! Evaluate trusted, revision-bound metrics without invoking a provider or tool.
use gateway_application::invocation_qualification::qualify;
use gateway_domain::invocation_qualification::{QualificationMeasurements, QualificationProfile};
fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args: Vec<_> = std::env::args().collect();
    if args.len() != 3 {
        return Err("usage: qualify-invocation PROFILE.json MEASUREMENTS.json".into());
    }
    let profile: QualificationProfile = serde_json::from_slice(&std::fs::read(&args[1])?)?;
    let measurements: QualificationMeasurements =
        serde_json::from_slice(&std::fs::read(&args[2])?)?;
    let decision = qualify(&profile, &measurements);
    println!("{}", serde_json::to_string_pretty(&decision)?);
    if !decision.eligible_for_promotion {
        std::process::exit(1);
    }
    Ok(())
}
