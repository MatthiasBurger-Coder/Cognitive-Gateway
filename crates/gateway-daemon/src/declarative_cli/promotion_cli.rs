use super::{CliError, Options, checked, decode, value};
use gateway_domain::procedure_promotion::{ProcedureVersion, PromotionJournal};
use gateway_registry::learned_procedures::LearnedProcedureRegistry;
use serde_json::{Value, json};

pub(super) fn execute(options: &Options) -> Result<(Value, i32), CliError> {
    let journal: PromotionJournal = decode(options.input("registry")?)?;
    let registry = checked(
        LearnedProcedureRegistry::from_journal(&journal),
        3,
        "INVALID_PROMOTION_JOURNAL",
    )?;
    let mut entries = Vec::new();
    for entry in registry.entries() {
        let executions = entry
            .executions()
            .values()
            .map(|e| {
                json!({
                    "request": e.request, "reserved_at": e.reserved_at,
                    "outcome": e.outcome, "outcome_evidence": e.outcome_evidence
                })
            })
            .collect::<Vec<_>>();
        entries.push(json!({
            "procedure": entry.procedure(), "version": ProcedureVersion::of(entry.procedure()),
            "state": entry.state(), "evaluation_digest": entry.evaluation(),
            "canary": entry.canary(), "predecessor": entry.predecessor(),
            "active_version": registry.active(entry.procedure().id()),
            "executions": executions, "history": entry.history()
        }));
    }
    Ok((
        json!({"schema_version": 1, "entries": entries, "journal": value(&journal)?}),
        0,
    ))
}
