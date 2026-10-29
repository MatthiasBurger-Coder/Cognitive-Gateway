use super::{Failure, GitHubReader, MAX_BODY};
use std::{
    io::{Read, Write},
    path::PathBuf,
    process::{Command, Stdio},
};

/// curl performs HTTPS GET only, no redirects/config inheritance/retries. The
/// token travels on stdin, never argv/environment of the child or diagnostics.
/// Runtime cancellation closes this server and its bounded (20s) outstanding GET.
pub struct CurlReader {
    executable: PathBuf,
    token: Option<String>,
}
impl CurlReader {
    pub fn new(executable: PathBuf, token: Option<String>) -> Result<Self, Failure> {
        if !executable.is_absolute()
            || token.as_ref().is_some_and(|s| {
                s.is_empty()
                    || s.len() > 4096
                    || !s
                        .bytes()
                        .all(|b| b.is_ascii_alphanumeric() || b"_-.".contains(&b))
            })
        {
            return Err(Failure::Configuration);
        }
        Ok(Self { executable, token })
    }
}
impl GitHubReader for CurlReader {
    fn get(&self, url: &str) -> Result<Vec<u8>, Failure> {
        if !url.starts_with("https://api.github.com/") || url.bytes().any(|b| b.is_ascii_control())
        {
            return Err(Failure::Request);
        }
        let mut child = Command::new(&self.executable)
            .env_clear()
            .args([
                "-q",
                "--config",
                "-",
                "--silent",
                "--proto",
                "=https",
                "--max-time",
                "20",
                "--connect-timeout",
                "5",
                "--max-filesize",
                &MAX_BODY.to_string(),
                "--header",
                "Accept: application/vnd.github+json",
                "--header",
                "X-GitHub-Api-Version: 2022-11-28",
                "--user-agent",
                "cg-github-mcp/1.0.0",
                "--write-out",
                "\n%{http_code}",
                "--url",
                url,
            ])
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .spawn()
            .map_err(|_| Failure::Unavailable)?;
        let result = (|| {
            if let Some(token) = &self.token {
                child
                    .stdin
                    .take()
                    .ok_or(Failure::Unavailable)?
                    .write_all(format!("header = \"Authorization: Bearer {token}\"\n").as_bytes())
                    .map_err(|_| Failure::Unavailable)?;
            } else {
                drop(child.stdin.take());
            }
            let mut bytes = Vec::new();
            child
                .stdout
                .take()
                .ok_or(Failure::Unavailable)?
                .take(MAX_BODY as u64 + 6)
                .read_to_end(&mut bytes)
                .map_err(|_| Failure::Unavailable)?;
            if bytes.len() > MAX_BODY + 4 {
                return Err(Failure::Payload);
            }
            if !child.wait().map_err(|_| Failure::Unavailable)?.success() {
                return Err(Failure::Unavailable);
            }
            if self
                .token
                .as_ref()
                .is_some_and(|s| bytes.windows(s.len()).any(|w| w == s.as_bytes()))
            {
                return Err(Failure::Payload);
            }
            let split = bytes
                .iter()
                .rposition(|b| *b == b'\n')
                .ok_or(Failure::Payload)?;
            match &bytes[split + 1..] {
                b"200" => {
                    bytes.truncate(split);
                    // JSON escapes must not hide a token that canonicalization
                    // would subsequently reveal to an MCP client.
                    let decoded =
                        crate::mcp_client::json::decode(&bytes).map_err(|_| Failure::Payload)?;
                    let canonical =
                        serde_json::to_string(&decoded).map_err(|_| Failure::Payload)?;
                    if self
                        .token
                        .as_ref()
                        .is_some_and(|token| canonical.contains(token))
                    {
                        return Err(Failure::Payload);
                    }
                    Ok(bytes)
                }
                b"401" => Err(Failure::Authentication),
                b"403" => Err(Failure::Permission),
                b"404" => Err(Failure::NotFound),
                b"429" => Err(Failure::RateLimited),
                _ => Err(Failure::Unavailable),
            }
        })();
        // Also reap on early read/write/size errors; raw curl output never escapes.
        let _ = child.kill();
        let _ = child.wait();
        result
    }
}
