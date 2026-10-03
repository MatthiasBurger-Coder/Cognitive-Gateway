use super::{CliError, Options, checked, decode, value};
use gateway_domain::{
    ReferenceId,
    learning::LearnedProcedure,
    procedure_evaluation::{CaseKind, EvaluationBundle, EvaluationDataset, counterfactuals},
};
use serde_json::Value;

pub(super) fn execute(options: &Options) -> Result<(Value, i32), CliError> {
    let bundle = if options.command == "replay" {
        let bundle: EvaluationBundle = decode(options.input("bundle")?)?;
        checked(bundle.validate(), 3, "INVALID_EVALUATION_BUNDLE")?;
        bundle
    } else {
        let procedure = checked(
            LearnedProcedure::from_json(&options.input("procedure")?.to_string()),
            3,
            "INVALID_PROCEDURE",
        )?;
        let mut dataset: EvaluationDataset = decode(options.input("dataset")?)?;
        checked(dataset.validate(), 3, "INVALID_DATASET")?;
        if options.command == "simulate" {
            let positives = dataset
                .cases
                .iter()
                .filter(|case| case.kind == CaseKind::HistoricalSuccess)
                .cloned()
                .collect::<Vec<_>>();
            for positive in positives {
                dataset.cases.extend(checked(
                    counterfactuals(&procedure, &positive),
                    3,
                    "INVALID_BASELINE",
                )?);
            }
        }
        let runtime = checked(
            ReferenceId::new(options.required("runtime-version")?),
            3,
            "INVALID_RUNTIME_VERSION",
        )?;
        checked(
            EvaluationBundle::evaluate(&procedure, dataset, runtime),
            3,
            "INVALID_EVALUATION",
        )?
    };
    let exit = if bundle.report.passed { 0 } else { 11 };
    Ok((value(&bundle)?, exit))
}
