//! Versioned, provider-neutral output definitions and verification handoff.
use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

use crate::{
    ReferenceId, ResolvedReference, SchemaVersion, SemanticTaskIR, SemanticVerificationCheck,
};

/// Closed JSON type algebra. Optional fields may be absent, never implicitly null.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "SCREAMING_SNAKE_CASE", deny_unknown_fields)]
pub enum OutputType {
    String {},
    Boolean {},
    Integer {},
    Number {},
    Array {
        items: Box<OutputType>,
    },
    Object {
        fields: BTreeMap<String, OutputField>,
    },
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct OutputField {
    pub required: bool,
    pub value_type: OutputType,
}

impl OutputType {
    /// Draft 2020-12 projection; adapter-specific envelopes belong outside domain.
    pub fn json_schema(&self) -> Value {
        match self {
            Self::String {} => json!({"type": "string"}),
            Self::Boolean {} => json!({"type": "boolean"}),
            Self::Integer {} => json!({"type": "integer"}),
            Self::Number {} => json!({"type": "number"}),
            Self::Array { items } => json!({"type": "array", "items": items.json_schema()}),
            Self::Object { fields } => {
                let properties: BTreeMap<_, _> = fields
                    .iter()
                    .map(|(name, field)| (name, field.value_type.json_schema()))
                    .collect();
                let required: Vec<_> = fields
                    .iter()
                    .filter(|(_, field)| field.required)
                    .map(|(name, _)| name)
                    .collect();
                json!({"type": "object", "properties": properties,
                    "required": required, "additionalProperties": false})
            }
        }
    }

    /// Validates already parsed JSON. JSON Schema integer semantics include 1.0.
    pub fn accepts(&self, value: &Value) -> bool {
        match self {
            Self::String {} => value.is_string(),
            Self::Boolean {} => value.is_boolean(),
            Self::Integer {} => value.as_number().is_some_and(|number| {
                number.is_i64()
                    || number.is_u64()
                    || number.as_f64().is_some_and(|n| n.fract() == 0.0)
            }),
            Self::Number {} => value.is_number(),
            Self::Array { items } => value
                .as_array()
                .is_some_and(|values| values.iter().all(|value| items.accepts(value))),
            Self::Object { fields } => value.as_object().is_some_and(|object| {
                object.keys().all(|name| fields.contains_key(name))
                    && fields.iter().all(|(name, field)| match object.get(name) {
                        Some(value) => field.value_type.accepts(value),
                        None => !field.required,
                    })
            }),
        }
    }
}

/// Definition loaded under the task's exact schema binding, never a latest alias.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct OutputContract {
    pub schema_version: SchemaVersion,
    pub schema: ResolvedReference<ReferenceId>,
    pub shape: OutputType,
}

impl OutputContract {
    pub fn json_schema(&self) -> Value {
        let mut schema = self.shape.json_schema();
        schema["$schema"] = json!("https://json-schema.org/draft/2020-12/schema");
        schema
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct VerificationRule {
    pub check: SemanticVerificationCheck,
    pub rule: ResolvedReference<ReferenceId>,
    pub verifier: ResolvedReference<ReferenceId>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct VerificationContract {
    pub schema_version: SchemaVersion,
    pub rules: Vec<VerificationRule>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum CheckOutcome {
    Passed,
    Failed,
    Indeterminate,
}

/// Distinct axes: successful execution does not imply valid or verified output.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum ResultValidity {
    ExecutionFailed,
    InvalidOutput,
    VerificationFailed,
    Unverified,
    Valid,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ContractError {
    Version,
    SchemaBinding,
    RuleSet,
    RuleBinding,
}

/// Admission checks structural compatibility. The host must additionally resolve
/// all schema/rule/verifier bindings and verify content digests before use.
pub fn validate_contracts(
    task: &SemanticTaskIR,
    output: &OutputContract,
    verification: &VerificationContract,
) -> Result<(), ContractError> {
    if output.schema_version != SchemaVersion::V1
        || verification.schema_version != SchemaVersion::V1
    {
        return Err(ContractError::Version);
    }
    if output.schema != task.data().output_contract.schema {
        return Err(ContractError::SchemaBinding);
    }
    let mut checks: Vec<_> = verification.rules.iter().map(|rule| rule.check).collect();
    checks.sort();
    if checks != task.data().verification_contract.checks {
        return Err(ContractError::RuleSet);
    }
    for rule in &verification.rules {
        for binding in [&rule.rule, &rule.verifier] {
            let digest = binding.digest.as_str();
            if binding.scope != output.schema.scope
                || digest.len() != 64
                || !digest
                    .bytes()
                    .all(|c| c.is_ascii_digit() || (b'a'..=b'f').contains(&c))
            {
                return Err(ContractError::RuleBinding);
            }
        }
    }
    Ok(())
}

/// Trusted host supplies outcomes for this exact task/result revision only.
/// Missing, indeterminate or extra reports cannot certify a result. Schema
/// validity is computed here, never accepted from a model's self-report.
pub fn assess_result(
    task: &SemanticTaskIR,
    output: &OutputContract,
    verification: &VerificationContract,
    execution: Result<&Value, ()>,
    outcomes: &BTreeMap<SemanticVerificationCheck, CheckOutcome>,
) -> Result<ResultValidity, ContractError> {
    validate_contracts(task, output, verification)?;
    let Ok(value) = execution else {
        return Ok(ResultValidity::ExecutionFailed);
    };
    if !output.shape.accepts(value) {
        return Ok(ResultValidity::InvalidOutput);
    }
    let required: Vec<_> = verification
        .rules
        .iter()
        .map(|rule| rule.check)
        .filter(|check| *check != SemanticVerificationCheck::OutputSchemaValid)
        .collect();
    if outcomes.keys().any(|check| !required.contains(check)) {
        return Ok(ResultValidity::Unverified);
    }
    if outcomes
        .values()
        .any(|outcome| *outcome == CheckOutcome::Failed)
    {
        return Ok(ResultValidity::VerificationFailed);
    }
    if required
        .iter()
        .any(|check| outcomes.get(check) != Some(&CheckOutcome::Passed))
    {
        return Ok(ResultValidity::Unverified);
    }
    Ok(ResultValidity::Valid)
}
