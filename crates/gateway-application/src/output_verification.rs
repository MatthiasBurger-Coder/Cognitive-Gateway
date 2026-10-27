//! Deterministic, side-effect-free verification over the exact admitted IR.
use crate::prompt_ir::{InvocationAdmissionPort, InvocationIr, validate_current};
use gateway_domain::{output_verification::*, prompt_ir::InvocationIrError, semantic_output::*, *};
use serde::{
    Deserialize,
    de::{self, MapAccess, SeqAccess, Visitor},
};
use serde_json::Value;
use std::collections::BTreeMap;
use std::sync::Arc;

pub use gateway_domain::output_parsing::MAX_OUTPUT_BYTES;
const MAX_DEPTH: usize = 64;

/// Duplicate keys must be rejected before Value can silently overwrite them.
struct StrictValue(Value);
impl<'de> Deserialize<'de> for StrictValue {
    fn deserialize<D: serde::Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        struct Strict;
        impl<'de> Visitor<'de> for Strict {
            type Value = StrictValue;
            fn expecting(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
                f.write_str("complete JSON without duplicate keys")
            }
            fn visit_bool<E: de::Error>(self, v: bool) -> Result<Self::Value, E> {
                Ok(StrictValue(v.into()))
            }
            fn visit_i64<E: de::Error>(self, v: i64) -> Result<Self::Value, E> {
                Ok(StrictValue(v.into()))
            }
            fn visit_u64<E: de::Error>(self, v: u64) -> Result<Self::Value, E> {
                Ok(StrictValue(v.into()))
            }
            fn visit_f64<E: de::Error>(self, v: f64) -> Result<Self::Value, E> {
                Ok(StrictValue(Value::from(v)))
            }
            fn visit_str<E: de::Error>(self, v: &str) -> Result<Self::Value, E> {
                Ok(StrictValue(v.into()))
            }
            fn visit_unit<E: de::Error>(self) -> Result<Self::Value, E> {
                Ok(StrictValue(Value::Null))
            }
            fn visit_seq<A: SeqAccess<'de>>(self, mut a: A) -> Result<Self::Value, A::Error> {
                let mut values = Vec::new();
                while let Some(StrictValue(value)) = a.next_element()? {
                    values.push(value);
                }
                Ok(StrictValue(Value::Array(values)))
            }
            fn visit_map<A: MapAccess<'de>>(self, mut a: A) -> Result<Self::Value, A::Error> {
                let mut values = serde_json::Map::new();
                while let Some(key) = a.next_key::<String>()? {
                    if values.contains_key(&key) {
                        return Err(de::Error::custom("duplicate key"));
                    }
                    let StrictValue(value) = a.next_value()?;
                    values.insert(key, value);
                }
                Ok(StrictValue(Value::Object(values)))
            }
        }
        d.deserialize_any(Strict)
    }
}
fn depth(v: &Value) -> usize {
    match v {
        Value::Array(a) => 1 + a.iter().map(depth).max().unwrap_or(0),
        Value::Object(o) => 1 + o.values().map(depth).max().unwrap_or(0),
        _ => 0,
    }
}
/// Cannot be constructed from caller-supplied Value or deserialized acceptance.
#[derive(Clone)]
pub struct ParsedOutput {
    data: OutputProposalData,
    digest: ContentDigest,
    raw: Arc<[u8]>,
}
impl std::fmt::Debug for ParsedOutput {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("ParsedOutput")
            .field("data", &self.data)
            .field("digest", &self.digest)
            .field("raw_len", &self.raw.len())
            .finish()
    }
}
impl ParsedOutput {
    pub fn data(&self) -> &OutputProposalData {
        &self.data
    }
    pub fn binding(&self) -> VerificationBinding {
        VerificationBinding {
            invocation: self.data.invocation.clone(),
            output_digest: self.digest.clone(),
        }
    }
    /// Exact bounded bytes consumed by this parser and bound by output_digest.
    /// Retention must preserve these bytes, including whitespace and key order;
    /// serializing the parsed fields is not a reconstruction of the same input.
    /// Loading retained bytes still requires parsing and current verification.
    pub fn raw_bytes(&self) -> &[u8] {
        &self.raw
    }
}
pub fn parse_output(raw: &[u8]) -> Result<ParsedOutput, VerificationError> {
    use VerificationError as E;
    if raw.len() > MAX_OUTPUT_BYTES {
        return Err(E::OutputLimitExceeded);
    }
    let StrictValue(value) = serde_json::from_slice(raw).map_err(|_| E::OutputParseFailed)?;
    if depth(&value) > MAX_DEPTH {
        return Err(E::OutputLimitExceeded);
    }
    let data: OutputProposalData =
        serde_json::from_value(value).map_err(|_| E::OutputParseFailed)?;
    if data.schema_version != SchemaVersion::V1
        || data.invocation.schema_version != SchemaVersion::V1
    {
        return Err(E::IncompatibleVersion);
    }
    Ok(ParsedOutput {
        data,
        digest: context_content_digest(raw),
        raw: Arc::from(raw),
    })
}

/// Trusted deterministic checks over *all* payload content, including undeclared
/// references, uncertainty markers, mutation boundaries and required fields.
/// No permissive implementation is supplied. Missing decisions fail closed.
#[derive(Debug, Clone)]
pub struct ConstraintReport {
    pub binding: VerificationBinding,
    pub policy: CheckOutcome,
    pub process: CheckOutcome,
    pub task_completeness: CheckOutcome,
    pub semantic_output: CheckOutcome,
    pub checks: BTreeMap<SemanticVerificationCheck, CheckOutcome>,
}
pub trait OutputVerificationPort: InvocationAdmissionPort {
    /// Required for evidence/claim checks. Absence fails closed; other tasks
    /// retain their existing contract. Hosts may opt in for any task.
    fn grounding(&self) -> Option<&dyn crate::grounding::GroundingEvidencePort> {
        None
    }
    /// Resolve exact entity pins from current trusted stores, never model labels.
    fn resolve_reference(
        &self,
        ir: &InvocationIr,
        reference: &OutputReference,
    ) -> Result<(), VerificationError>;
    fn evaluate(
        &self,
        ir: &InvocationIr,
        output: &ParsedOutput,
    ) -> Result<ConstraintReport, VerificationError>;
}
#[derive(Debug, Clone)]
pub struct VerifiedOutput {
    output: ParsedOutput,
    record: VerificationRecord,
}
impl VerifiedOutput {
    pub fn output(&self) -> &ParsedOutput {
        &self.output
    }
    pub fn record(&self) -> &VerificationRecord {
        &self.record
    }
}
fn current(ir: &InvocationIr, host: &dyn OutputVerificationPort) -> Result<(), VerificationError> {
    validate_current(ir, host).map_err(|e| match e {
        InvocationIrError::StaleInput => VerificationError::StaleContext,
        InvocationIrError::AuthenticationUnavailable => VerificationError::VerificationUnavailable,
        _ => VerificationError::AuthorityDenied,
    })
}
fn check(
    ir: &InvocationIr,
    output: &ParsedOutput,
    host: &dyn OutputVerificationPort,
) -> Result<(), VerificationError> {
    use VerificationError as E;
    let d = &output.data;
    let contract = &ir.prepared().invocation().data().output;
    validate_output_proposal(&ir.identity(), contract, d)?;
    current(ir, host)?;
    for reference in &d.references {
        host.resolve_reference(ir, reference)?;
    }
    crate::output_parsing::validate_tools(ir, d)?;
    let report = host.evaluate(ir, output)?;
    if report.binding != output.binding() {
        return Err(E::CorrelationMismatch);
    }
    for outcome in [
        report.policy,
        report.process,
        report.task_completeness,
        report.semantic_output,
    ] {
        match outcome {
            CheckOutcome::Passed => (),
            CheckOutcome::Failed => return Err(E::ConstraintViolation),
            CheckOutcome::Indeterminate => return Err(E::VerificationUnavailable),
        }
    }
    let handoff = ir.prompt().handoff().to_value();
    let verification: VerificationContract =
        serde_json::from_value(handoff["verification"].clone())
            .map_err(|_| E::VerificationUnavailable)?;
    let grounding_required = verification.rules.iter().any(|rule| {
        matches!(
            rule.check,
            SemanticVerificationCheck::EvidenceRequired
                | SemanticVerificationCheck::AllClaimsSupported
        )
    });
    if grounding_required || host.grounding().is_some() {
        let evidence = host.grounding().ok_or(E::VerificationUnavailable)?;
        crate::grounding::verify_grounding(ir, output, evidence).map_err(|error| match error {
            gateway_domain::grounding::GroundingError::Unavailable => E::VerificationUnavailable,
            gateway_domain::grounding::GroundingError::CorrelationMismatch => {
                E::CorrelationMismatch
            }
            error => E::GroundingFailed(error),
        })?;
    }
    let validity = assess_result(
        ir.prompt().handoff().task(),
        contract,
        &verification,
        Ok(&d.result),
        &report.checks,
    )
    .map_err(|_| E::VerificationUnavailable)?;
    if validity != ResultValidity::Valid {
        return Err(E::SemanticVerificationFailed);
    }
    // Detect revocation or context changes while reference/constraint checks ran.
    current(ir, host)
}
/// Critics are audit signals only. Every rejection retains exact invocation and
/// raw-output digest correlation; no API here dispatches tools or external effects.
pub fn verify_output(
    ir: &InvocationIr,
    output: ParsedOutput,
    host: &dyn OutputVerificationPort,
    critics: Vec<CheckOutcome>,
) -> Result<VerifiedOutput, Box<VerificationRecord>> {
    let diagnostic = check(ir, &output, host).err();
    let record = VerificationRecord {
        admitted_invocation: ir.identity(),
        schema_version: SchemaVersion::V1,
        binding: output.binding(),
        validity: if diagnostic.is_none() {
            ResultValidity::Valid
        } else {
            ResultValidity::VerificationFailed
        },
        diagnostic,
        critics,
    };
    if diagnostic.is_some() {
        return Err(Box::new(record));
    }
    Ok(VerifiedOutput { output, record })
}
