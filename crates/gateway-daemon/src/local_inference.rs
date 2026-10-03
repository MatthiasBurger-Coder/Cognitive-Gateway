//! HTTP adapter to the provider-neutral local cognitive service.
//! curl supplies HTTP framing and a bounded total deadline outside the core.
use gateway_application::local_inference::{
    LocalInferenceError, LocalInferencePort, LocalInferenceProposal, LocalInferenceRequest,
};
use std::io::Write;
use std::process::{Command, Stdio};

pub struct HttpLocalInferenceAdapter {
    endpoint: String,
    timeout_seconds: u32,
}

impl HttpLocalInferenceAdapter {
    pub fn new(endpoint: String, timeout_seconds: u32) -> Result<Self, LocalInferenceError> {
        if !endpoint.starts_with("http://") && !endpoint.starts_with("https://")
            || endpoint.contains(['\r', '\n', '?', '#'])
            || !(1..=3600).contains(&timeout_seconds)
        {
            return Err(LocalInferenceError::InvalidRequest);
        }
        Ok(Self {
            endpoint: endpoint.trim_end_matches('/').to_owned(),
            timeout_seconds,
        })
    }

    pub fn from_environment() -> Result<Self, LocalInferenceError> {
        Self::new(
            std::env::var("CG_LOCAL_INFERENCE_ENDPOINT")
                .unwrap_or_else(|_| "http://127.0.0.1:8091".into()),
            std::env::var("CG_LOCAL_INFERENCE_TIMEOUT")
                .unwrap_or_else(|_| "180".into())
                .parse()
                .map_err(|_| LocalInferenceError::InvalidRequest)?,
        )
    }
}

impl LocalInferencePort for HttpLocalInferenceAdapter {
    fn infer(
        &self,
        request: &LocalInferenceRequest,
    ) -> Result<LocalInferenceProposal, LocalInferenceError> {
        if request.schema_version != "1.0"
            || request.role.is_empty()
            || request.prompt.is_empty()
            || request.prompt.len() > 16000
            || !request.output_schema.is_object()
        {
            return Err(LocalInferenceError::InvalidRequest);
        }
        let data = serde_json::to_vec(request).map_err(|_| LocalInferenceError::InvalidRequest)?;
        let mut child = Command::new("curl")
            .args([
                "--silent",
                "--fail",
                "--max-time",
                &self.timeout_seconds.to_string(),
                "--max-filesize",
                "2097152",
                "--header",
                "Content-Type: application/json",
                "--data-binary",
                "@-",
                "--url",
                &format!("{}/v1/infer", self.endpoint),
            ])
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .spawn()
            .map_err(|_| LocalInferenceError::Unavailable)?;
        let write_result = child
            .stdin
            .take()
            .ok_or(LocalInferenceError::Unavailable)?
            .write_all(&data);
        let output = child
            .wait_with_output()
            .map_err(|_| LocalInferenceError::Unavailable)?;
        if write_result.is_err() || !output.status.success() {
            return Err(LocalInferenceError::Unavailable);
        }
        let proposal: LocalInferenceProposal = serde_json::from_slice(&output.stdout)
            .map_err(|_| LocalInferenceError::InvalidProposal)?;
        if proposal.schema_version != "1.0"
            || proposal.kind != "proposal"
            || proposal.model_id.is_empty()
            || !proposal.artifact_digest.starts_with("sha256:")
            || proposal.artifact_digest.len() != 71
            || !proposal.artifact_digest[7..]
                .bytes()
                .all(|byte| byte.is_ascii_hexdigit())
        {
            return Err(LocalInferenceError::InvalidProposal);
        }
        Ok(proposal)
    }
}
