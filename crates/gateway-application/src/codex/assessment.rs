//! Shared canonical assessment mapping used by inbound adapters.
use crate::DeclarativeSituationApplication;
use gateway_domain::*;
use serde::{Deserialize, Serialize};

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AssessmentInput {
    pub schema_version: u32,
    pub scope: ContextScopeId,
    pub operating_mode: OperatingMode,
    pub execution_profile: ExecutionProfile,
    pub context: DeclarativeContext,
    pub observed_state_id: ObservedStateId,
    pub situation_id: SituationId,
    pub records: ObservationEvidenceSet,
    #[serde(default)]
    pub unknown_subjects: Vec<String>,
    pub intent: Option<Intent>,
}
#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Assessment {
    pub schema_version: u32,
    pub scope: ContextScopeId,
    pub operating_mode: OperatingMode,
    pub execution_profile: ExecutionProfile,
    pub document: DeclarativeContextSituationDocument,
}
impl AssessmentInput {
    pub fn assess(self) -> Result<Assessment, super::FacadeError> {
        if self.schema_version != 1 {
            return Err(super::FacadeError::UnsupportedVersion);
        }
        let app = DeclarativeSituationApplication::new();
        let subjects = self
            .unknown_subjects
            .iter()
            .map(|s| s.parse::<SubjectPath>())
            .collect::<Result<Vec<_>, _>>()
            .map_err(|_| super::FacadeError::InvalidInput)?;
        let normalization = NormalizationInput::new(self.records.clone())
            .with_unknown_subjects(subjects)
            .map_err(|_| super::FacadeError::InvalidInput)?;
        let current = app
            .normalize_current_state(self.observed_state_id, normalization)
            .map_err(|_| super::FacadeError::InvalidInput)?;
        let situation = app
            .assess_situation(
                SituationAssemblyInput::new(current.clone()).with_records(self.records.clone()),
                self.situation_id,
            )
            .map_err(|_| super::FacadeError::InvalidInput)?;
        let document = app
            .validate_declarative_context(
                self.context,
                self.intent,
                Some(self.records),
                current,
                situation,
            )
            .map_err(|_| super::FacadeError::InvalidInput)?;
        Ok(Assessment {
            schema_version: 1,
            scope: self.scope,
            operating_mode: self.operating_mode,
            execution_profile: self.execution_profile,
            document,
        })
    }
}
