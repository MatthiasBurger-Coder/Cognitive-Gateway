use super::URI;
use crate::mcp_client::{
    Error, ProtocolResult,
    mapping::{ConnectorResultNormalizer, MappingRecord},
    normalization::{AcquisitionPort, CanonicalResultNormalizer, NormalizationProfile},
};
use gateway_application::connectors::*;
use gateway_domain::*;

/// Provider-specific identity checks, followed by the unchanged CG normalizer.
pub struct DocumentResultNormalizer(CanonicalResultNormalizer);
impl DocumentResultNormalizer {
    pub fn new(acquisition: Box<dyn AcquisitionPort>) -> Result<Self, Error> {
        Ok(Self(CanonicalResultNormalizer::new(
            NormalizationProfile {
                source: SourceId::new("source.conformance.document").expect("static source"),
                source_kind: SourceKind::Tool,
                object_field: "uri".into(),
                revision_field: Some("version".into()),
                digest_field: None,
                fields: [(
                    "body".into(),
                    SubjectPath::new(["document", "content"]).expect("static subject"),
                )]
                .into(),
            },
            acquisition,
        )?))
    }
}
impl ConnectorResultNormalizer for DocumentResultNormalizer {
    fn normalize(
        &self,
        d: &ConnectorDispatch,
        source: Option<&SourceId>,
        mapping: &MappingRecord,
        result: ProtocolResult,
    ) -> Result<ConnectorObservation, Error> {
        if mapping.name != "documents.fetch" || d.request.capability.as_str() != "document.read" {
            return Err(Error::Schema);
        }
        match &result {
            ProtocolResult::Tool {
                content,
                is_error: true,
                ..
            } if content.len() == 1
                && content[0]["type"] == "text"
                && content[0]["text"] == "DOCUMENT_NOT_FOUND" =>
            {
                return Err(Error::SessionDenied(ConnectorFailureCode::NotFound));
            }
            ProtocolResult::Tool {
                structured: Some(data),
                is_error: false,
                ..
            } if data["uri"] == URI
                && d.request.arguments
                    == [(
                        NonEmptyText::new("key").expect("static key"),
                        TypedValue::String(NonEmptyText::new("guide").expect("static key")),
                    )]
                    .into() => {}
            _ => return Err(Error::Schema),
        }
        self.0.normalize(d, source, mapping, result)
    }
}
