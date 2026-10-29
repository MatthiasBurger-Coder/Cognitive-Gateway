//! Provider projection reuses the canonical CG-06 evidence validator unchanged.
use super::{Failure, GitHubConnector, GitHubReader};
use crate::mcp_client::{
    Error, ProtocolResult,
    mapping::{ConnectorResultNormalizer, MappingRecord},
    normalization::{
        Acquisition, AcquisitionPort, CanonicalResultNormalizer, NormalizationProfile,
    },
};
use gateway_application::connectors::*;
use gateway_domain::*;
use std::sync::Arc;

struct NoIo;
impl GitHubReader for NoIo {
    fn get(&self, _: &str) -> Result<Vec<u8>, Failure> {
        Err(Failure::Unavailable)
    }
}
struct Collected(Acquisition);
impl AcquisitionPort for Collected {
    fn collect(&self, _: &ConnectorDispatch) -> Result<Acquisition, Error> {
        Ok(self.0.clone())
    }
}
/// Bound repository plus trusted acquisition metadata. This adapter grants no
/// authentication/trust/quality upgrade and is not a qualified goal-read profile.
pub struct GitHubResultNormalizer {
    connector: GitHubConnector<NoIo>,
    acquisition: Arc<dyn AcquisitionPort>,
}
impl GitHubResultNormalizer {
    pub fn new(repository: &str, acquisition: Arc<dyn AcquisitionPort>) -> Result<Self, Failure> {
        Ok(Self {
            connector: GitHubConnector::new(repository, NoIo)?,
            acquisition,
        })
    }
}
impl ConnectorResultNormalizer for GitHubResultNormalizer {
    fn normalize(
        &self,
        dispatch: &ConnectorDispatch,
        source: Option<&SourceId>,
        mapping: &MappingRecord,
        result: ProtocolResult,
    ) -> Result<ConnectorObservation, Error> {
        let args: serde_json::Map<String, serde_json::Value> = dispatch
            .request
            .arguments
            .iter()
            .map(|(key, value)| {
                let value = match value {
                    TypedValue::String(s) => serde_json::json!(s.as_str()),
                    TypedValue::Integer(i) => serde_json::json!(i),
                    _ => return Err(Error::Schema),
                };
                Ok((key.as_str().into(), value))
            })
            .collect::<Result<_, Error>>()?;
        let (route, url) = self
            .connector
            .request_url(
                mapping.capability.as_str(),
                &serde_json::Value::Object(args),
            )
            .map_err(|_| Error::Schema)?;
        let expected_source = SourceId::new(format!("source.{}", mapping.capability.as_str()))
            .map_err(|_| Error::Schema)?;
        if mapping.name != mapping.capability.as_str()
            || mapping.source.as_ref() != Some(&expected_source)
        {
            return Err(Error::Schema);
        }
        if let ProtocolResult::Tool {
            content,
            is_error: true,
            ..
        } = &result
        {
            let code = if content.len() == 1 && content[0]["type"] == "text" {
                content[0]["text"].as_str()
            } else {
                None
            };
            return Err(match code {
                Some("CONNECTOR_AUTH_FAILED") => Error::Authentication,
                Some("CONNECTOR_PERMISSION_DENIED") => {
                    Error::SessionDenied(ConnectorFailureCode::PermissionDenied)
                }
                Some("CONNECTOR_NOT_FOUND") => Error::SessionDenied(ConnectorFailureCode::NotFound),
                Some("CONNECTOR_RATE_LIMITED") => {
                    Error::SessionDenied(ConnectorFailureCode::RateLimited)
                }
                Some("CONNECTOR_UNAVAILABLE") => Error::Unavailable,
                _ => Error::Schema,
            });
        }
        let partial = match &result {
            ProtocolResult::Tool {
                structured: Some(data),
                is_error: false,
                ..
            } if data["object"] == url => data["partial"].as_bool().ok_or(Error::Schema)?,
            _ => return Err(Error::Schema),
        };
        let mut acquisition = self.acquisition.collect(dispatch)?;
        acquisition.partial |= route.partial || partial;
        let profile = NormalizationProfile {
            source: expected_source,
            source_kind: SourceKind::Tool,
            object_field: "object".into(),
            revision_field: Some("revision".into()),
            digest_field: None,
            fields: [
                (
                    "content".into(),
                    SubjectPath::new(["github", "content"]).map_err(|_| Error::Configuration)?,
                ),
                (
                    "partial".into(),
                    SubjectPath::new(["github", "partial"]).map_err(|_| Error::Configuration)?,
                ),
            ]
            .into(),
        };
        CanonicalResultNormalizer::new(profile, Box::new(Collected(acquisition)))?
            .normalize(dispatch, source, mapping, result)
    }
}
