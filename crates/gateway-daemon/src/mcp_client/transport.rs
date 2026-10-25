//! Bounded newline JSON-RPC transport. No inherited environment or stderr export.
use super::{CallContext, Error, Limits};
use std::{collections::BTreeMap, path::PathBuf, process::Stdio, time::Duration};
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    process::{Child, ChildStdin, ChildStdout, Command},
    runtime::{Builder, Runtime},
};

/// Infrastructure-only port: implementations must honor the context on every IO.
pub trait Transport: Send {
    fn send(&mut self, frame: &[u8], context: &CallContext) -> Result<(), Error>;
    fn receive(&mut self, context: &CallContext) -> Result<Vec<u8>, Error>;
    fn close(&mut self);
}
pub trait TransportFactory: Send + Sync {
    fn open(&self, context: &CallContext, limits: &Limits) -> Result<Box<dyn Transport>, Error>;
    fn contains_secret(&self, _candidate: &str) -> bool {
        false
    }
    fn connection_closed(&self) {}
    fn permits_tool(&self, _connector: &str, _server: &str, _tool: &str) -> Result<(), Error> {
        Ok(())
    }
}

/// Reloadable in-process secret store adapter. Production deployments can
/// implement ConnectorSecretResolver with an OS keyring or secret manager.
pub struct ReloadableCredentialResolver {
    profiles:
        std::sync::RwLock<BTreeMap<String, gateway_application::connectors::ResolvedCredential>>,
}
/// Explicit value redactor for adapters that must retain provider diagnostics.
/// It owns copies of secret values and exposes only the redacted projection.
pub struct SecretRedactor(Vec<String>);
impl SecretRedactor {
    pub fn new<'a>(secrets: impl IntoIterator<Item = &'a str>) -> Self {
        Self(
            secrets
                .into_iter()
                .filter(|s| !s.is_empty())
                .map(str::to_owned)
                .collect(),
        )
    }
    pub fn redact(&self, input: &str) -> String {
        self.0.iter().fold(input.to_owned(), |text, secret| {
            text.replace(secret, "[REDACTED]")
        })
    }
}
impl ReloadableCredentialResolver {
    pub fn new() -> Self {
        Self {
            profiles: std::sync::RwLock::new(BTreeMap::new()),
        }
    }
    pub fn replace(
        &self,
        profile: &gateway_application::connectors::CredentialProfileRef,
        credential: gateway_application::connectors::ResolvedCredential,
    ) {
        self.profiles
            .write()
            .unwrap_or_else(|e| e.into_inner())
            .insert(profile.as_str().to_owned(), credential);
    }
    pub fn remove(&self, profile: &gateway_application::connectors::CredentialProfileRef) {
        self.profiles
            .write()
            .unwrap_or_else(|e| e.into_inner())
            .remove(profile.as_str());
    }
}
impl Default for ReloadableCredentialResolver {
    fn default() -> Self {
        Self::new()
    }
}
impl gateway_application::connectors::ConnectorSecretResolver for ReloadableCredentialResolver {
    fn resolve(
        &self,
        profile: &gateway_application::connectors::CredentialProfileRef,
    ) -> Result<
        gateway_application::connectors::ResolvedCredential,
        gateway_application::connectors::ConnectorFailureCode,
    > {
        self.profiles
            .read()
            .unwrap_or_else(|e| e.into_inner())
            .get(profile.as_str())
            .map(|c| {
                gateway_application::connectors::ResolvedCredential::new(
                    c.environment.clone(),
                    c.scope.clone(),
                    c.generation,
                )
            })
            .ok_or(gateway_application::connectors::ConnectorFailureCode::CredentialUnavailable)
    }
}

/// Production composition wrapper: resolves on each connection, validates the
/// configured identity and enforces the resolved tool allowlist before dispatch.
pub struct CredentialedStdioFactory {
    config: StdioConfig,
    resolver: std::sync::Arc<dyn gateway_application::connectors::ConnectorSecretResolver>,
    profile: gateway_application::connectors::CredentialProfileRef,
    connector: String,
    server: String,
    active_scope: std::sync::Mutex<Option<gateway_application::connectors::CredentialScope>>,
    active_secrets: std::sync::Mutex<Vec<String>>,
}
impl CredentialedStdioFactory {
    pub fn new(
        config: StdioConfig,
        resolver: std::sync::Arc<dyn gateway_application::connectors::ConnectorSecretResolver>,
        profile: gateway_application::connectors::CredentialProfileRef,
        connector: impl Into<String>,
        server: impl Into<String>,
    ) -> Self {
        Self {
            config,
            resolver,
            profile,
            connector: connector.into(),
            server: server.into(),
            active_scope: std::sync::Mutex::new(None),
            active_secrets: std::sync::Mutex::new(Vec::new()),
        }
    }
}
impl TransportFactory for CredentialedStdioFactory {
    fn open(&self, context: &CallContext, limits: &Limits) -> Result<Box<dyn Transport>, Error> {
        use gateway_application::connectors::ConnectorFailureCode as C;
        self.connection_closed();
        if self
            .config
            .environment
            .keys()
            .any(|key| credential_like_key(key))
        {
            return Err(Error::Configuration);
        }
        let credential = self
            .resolver
            .resolve(&self.profile)
            .map_err(|code| match code {
                C::AuthenticationFailed => Error::Authentication,
                _ => Error::CredentialUnavailable,
            })?;
        if credential.scope.connector.as_str() != self.connector
            || credential.scope.server.as_str() != self.server
        {
            return Err(Error::CredentialScopeDenied);
        }
        let config = StdioConfig {
            executable: self.config.executable.clone(),
            arguments: self.config.arguments.clone(),
            directory: self.config.directory.clone(),
            environment: self.config.environment.clone(),
        }
        .with_credential_environment(&credential)?;
        let transport = config.open_resolved(context, limits)?;
        *self
            .active_secrets
            .lock()
            .unwrap_or_else(|e| e.into_inner()) = credential
            .environment
            .values()
            .filter(|v| !v.is_empty())
            .cloned()
            .collect();
        *self.active_scope.lock().unwrap_or_else(|e| e.into_inner()) = Some(credential.scope);
        Ok(transport)
    }
    fn permits_tool(&self, connector: &str, server: &str, tool: &str) -> Result<(), Error> {
        if connector != self.connector || server != self.server {
            return Err(Error::CredentialScopeDenied);
        }
        let scope = self.active_scope.lock().unwrap_or_else(|e| e.into_inner());
        if scope
            .as_ref()
            .is_some_and(|s| s.permits(connector, server, tool))
        {
            Ok(())
        } else {
            Err(Error::CredentialScopeDenied)
        }
    }
    fn contains_secret(&self, candidate: &str) -> bool {
        self.active_secrets
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .iter()
            .any(|secret| candidate.contains(secret))
    }
    fn connection_closed(&self) {
        self.active_secrets
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .clear();
        *self.active_scope.lock().unwrap_or_else(|e| e.into_inner()) = None;
    }
}

/// Trusted operator configuration; arguments/environment are never taken from proposals.
/// No Debug implementation: configuration may hold adapter-resolved credentials.
pub struct StdioConfig {
    pub executable: PathBuf,
    pub arguments: Vec<String>,
    pub directory: PathBuf,
    pub environment: BTreeMap<String, String>,
}
impl StdioConfig {
    /// Build an effective launch configuration with only explicitly scoped
    /// credential variables. Credential keys may not replace operator config.
    pub fn with_credential_environment(
        mut self,
        credentials: &gateway_application::connectors::ResolvedCredential,
    ) -> Result<Self, Error> {
        for (key, value) in &credentials.environment {
            if !valid_credential_key(key) || value.is_empty() || self.environment.contains_key(key)
            {
                return Err(Error::Configuration);
            }
            self.environment.insert(key.clone(), value.clone());
        }
        Ok(self)
    }
}
fn valid_credential_key(key: &str) -> bool {
    !key.is_empty()
        && key.len() <= 128
        && key
            .bytes()
            .all(|b| b.is_ascii_uppercase() || b.is_ascii_digit() || b == b'_')
        && credential_like_key(key)
}
fn credential_like_key(key: &str) -> bool {
    [
        "TOKEN",
        "PASSWORD",
        "SECRET",
        "API_KEY",
        "AUTH",
        "CREDENTIAL",
        "PRIVATE_KEY",
    ]
    .iter()
    .any(|marker| key.contains(marker))
}
struct StdioTransport {
    executor: Runtime,
    child: Child,
    input: Option<ChildStdin>,
    output: ChildStdout,
    pending: Vec<u8>,
    max_frame: usize,
    shutdown: Duration,
}
impl TransportFactory for StdioConfig {
    fn open(&self, context: &CallContext, limits: &Limits) -> Result<Box<dyn Transport>, Error> {
        if self.environment.keys().any(|key| credential_like_key(key)) {
            return Err(Error::Configuration);
        }
        self.open_resolved(context, limits)
    }
}
impl StdioConfig {
    fn open_resolved(
        &self,
        context: &CallContext,
        limits: &Limits,
    ) -> Result<Box<dyn Transport>, Error> {
        context.check()?;
        if !self.executable.is_absolute() || !self.directory.is_absolute() {
            return Err(Error::Configuration);
        }
        let executor = Builder::new_current_thread()
            .enable_all()
            .build()
            .map_err(|_| Error::Unavailable)?;
        let child = {
            let _entered = executor.enter();
            Command::new(&self.executable)
                .args(&self.arguments)
                .current_dir(&self.directory)
                .env_clear()
                .envs(&self.environment)
                .stdin(Stdio::piped())
                .stdout(Stdio::piped())
                .stderr(Stdio::null())
                .kill_on_drop(true)
                .spawn()
                .map_err(|_| Error::Unavailable)?
        };
        let mut child = child;
        let input = child.stdin.take();
        let output = child.stdout.take().ok_or(Error::Unavailable)?;
        Ok(Box::new(StdioTransport {
            executor,
            child,
            input,
            output,
            pending: Vec::new(),
            max_frame: limits.frame_bytes,
            shutdown: limits.shutdown,
        }))
    }
}
impl Transport for StdioTransport {
    fn send(&mut self, frame: &[u8], context: &CallContext) -> Result<(), Error> {
        if frame.len() > self.max_frame || frame.contains(&b'\n') {
            return Err(Error::Limit);
        }
        let input = self.input.as_mut().ok_or(Error::Unavailable)?;
        let mut bytes = frame.to_vec();
        bytes.push(b'\n');
        let mut offset = 0;
        while offset < bytes.len() {
            context.check()?;
            match self.executor.block_on(async {
                tokio::time::timeout(context.slice(), input.write(&bytes[offset..])).await
            }) {
                Ok(Ok(0)) | Ok(Err(_)) => return Err(Error::Unavailable),
                Ok(Ok(n)) => offset += n,
                Err(_) => (),
            }
        }
        Ok(())
    }
    fn receive(&mut self, context: &CallContext) -> Result<Vec<u8>, Error> {
        loop {
            context.check()?;
            if let Some(end) = self.pending.iter().position(|b| *b == b'\n') {
                let mut line: Vec<u8> = self.pending.drain(..=end).collect();
                line.pop();
                if line.len() > self.max_frame {
                    return Err(Error::Limit);
                }
                return Ok(line);
            }
            if self.pending.len() > self.max_frame {
                return Err(Error::Limit);
            }
            let mut chunk = [0; 4096];
            match self.executor.block_on(async {
                tokio::time::timeout(context.slice(), self.output.read(&mut chunk)).await
            }) {
                Ok(Ok(0)) | Ok(Err(_)) => return Err(Error::Unavailable),
                Ok(Ok(n)) => self.pending.extend_from_slice(&chunk[..n]),
                Err(_) => (),
            }
        }
    }
    fn close(&mut self) {
        self.input.take();
        self.executor.block_on(async {
            if tokio::time::timeout(self.shutdown, self.child.wait())
                .await
                .is_err()
            {
                let _ = self.child.start_kill();
                let _ = tokio::time::timeout(self.shutdown, self.child.wait()).await;
            }
        });
    }
}
impl Drop for StdioTransport {
    fn drop(&mut self) {
        self.close();
    }
}

#[cfg(test)]
mod credential_tests {
    use super::*;
    use gateway_application::connectors::{
        ConnectorSecretResolver, CredentialScope, ResolvedCredential,
    };
    use gateway_domain::NonEmptyText;

    fn credential(environment: &[(&str, &str)]) -> ResolvedCredential {
        ResolvedCredential::new(
            environment
                .iter()
                .map(|(k, v)| ((*k).to_owned(), (*v).to_owned()))
                .collect(),
            CredentialScope {
                connector: NonEmptyText::new("github").unwrap(),
                server: NonEmptyText::new("github-mcp").unwrap(),
                tools: [NonEmptyText::new("issues.read").unwrap()]
                    .into_iter()
                    .collect(),
            },
            4,
        )
    }

    #[test]
    fn credential_binding_accepts_secret_keys_without_debug_or_serialization() {
        let config = StdioConfig {
            executable: "/bin/true".into(),
            arguments: vec![],
            directory: "/tmp".into(),
            environment: BTreeMap::new(),
        }
        .with_credential_environment(&credential(&[("GITHUB_TOKEN", "canary-secret")]))
        .unwrap();
        assert_eq!(
            config.environment.get("GITHUB_TOKEN").map(String::as_str),
            Some("canary-secret")
        );
        assert!(!std::any::type_name_of_val(&config).contains("canary-secret"));
        let scope = credential(&[]).scope;
        assert!(scope.permits("github", "github-mcp", "issues.read"));
        assert!(!scope.permits("github", "github-mcp", "issues.write"));
        assert!(!scope.permits("other", "github-mcp", "issues.read"));
    }

    #[test]
    fn credential_binding_rejects_broad_keys_empty_values_and_operator_override() {
        let base = || StdioConfig {
            executable: "/bin/true".into(),
            arguments: vec![],
            directory: "/tmp".into(),
            environment: BTreeMap::new(),
        };
        assert!(
            base()
                .with_credential_environment(&credential(&[("PATH", "x")]))
                .is_err()
        );
        assert!(
            base()
                .with_credential_environment(&credential(&[("API_KEY", "")]))
                .is_err()
        );
        let mut configured = base();
        configured
            .environment
            .insert("GITHUB_TOKEN".into(), "operator-value".into());
        assert!(
            configured
                .with_credential_environment(&credential(&[("GITHUB_TOKEN", "secret")]))
                .is_err()
        );
    }

    #[test]
    fn resolver_reload_is_profile_scoped_and_missing_profiles_fail_closed() {
        use gateway_application::connectors::{ConnectorFailureCode, CredentialProfileRef};
        let resolver = ReloadableCredentialResolver::new();
        let profile = CredentialProfileRef::new(NonEmptyText::new("profile.github").unwrap());
        assert_eq!(
            resolver.resolve(&profile).err().unwrap(),
            ConnectorFailureCode::CredentialUnavailable
        );
        resolver.replace(&profile, credential(&[("GITHUB_TOKEN", "old-secret")]));
        assert_eq!(resolver.resolve(&profile).unwrap().generation, 4);
        resolver.replace(
            &profile,
            ResolvedCredential::new(
                [("GITHUB_TOKEN".to_owned(), "new-secret".to_owned())]
                    .into_iter()
                    .collect(),
                credential(&[]).scope,
                5,
            ),
        );
        let current = resolver.resolve(&profile).unwrap();
        assert_eq!(current.generation, 5);
        assert_eq!(current.environment["GITHUB_TOKEN"], "new-secret");
        resolver.remove(&profile);
        assert_eq!(
            resolver.resolve(&profile).err().unwrap(),
            ConnectorFailureCode::CredentialUnavailable
        );
    }

    #[test]
    fn redactor_removes_canary_secrets_from_adapter_diagnostics() {
        let redactor = SecretRedactor::new(["token-abc", "password xyz"]);
        let safe = redactor.redact("authorization token-abc rejected; password xyz invalid");
        assert_eq!(
            safe,
            "authorization [REDACTED] rejected; [REDACTED] invalid"
        );
        assert!(!safe.contains("token-abc"));
        assert!(!safe.contains("password xyz"));
    }

    #[test]
    fn credentialed_factory_denies_unlisted_tool_and_wrong_server() {
        use gateway_application::connectors::{ConnectorFailureCode, CredentialProfileRef};
        let profile = CredentialProfileRef::new(NonEmptyText::new("profile.github").unwrap());
        let resolver = std::sync::Arc::new(ReloadableCredentialResolver::new());
        resolver.replace(&profile, credential(&[]));
        let config = StdioConfig {
            executable: "/bin/true".into(),
            arguments: vec![],
            directory: "/tmp".into(),
            environment: BTreeMap::new(),
        };
        let factory =
            CredentialedStdioFactory::new(config, resolver, profile, "github", "github-mcp");
        *factory.active_scope.lock().unwrap() = Some(credential(&[]).scope);
        assert!(
            factory
                .permits_tool("github", "github-mcp", "issues.read")
                .is_ok()
        );
        assert_eq!(
            factory.permits_tool("github", "github-mcp", "issues.write"),
            Err(Error::CredentialScopeDenied)
        );
        assert_eq!(
            factory.permits_tool("github", "other", "issues.read"),
            Err(Error::CredentialScopeDenied)
        );
        *factory.active_secrets.lock().unwrap() = vec!["token-canary".to_owned()];
        assert!(factory.contains_secret("provider echoed token-canary in content"));
        assert!(!factory.contains_secret("ordinary provider content"));
        assert_eq!(
            Error::Authentication
                .failure(
                    &NonEmptyText::new("corr").unwrap(),
                    gateway_application::connectors::ConnectorEffect::NotDispatched
                )
                .code,
            ConnectorFailureCode::AuthenticationFailed
        );
    }

    #[test]
    fn factory_rejects_unprofiled_auth_environment() {
        use gateway_application::connectors::CredentialProfileRef;
        let profile = CredentialProfileRef::new(NonEmptyText::new("profile.github").unwrap());
        let resolver = std::sync::Arc::new(ReloadableCredentialResolver::new());
        resolver.replace(&profile, credential(&[]));
        let mut environment = BTreeMap::new();
        environment.insert("GH_TOKEN".to_owned(), "unscoped-secret".to_owned());
        let factory = CredentialedStdioFactory::new(
            StdioConfig {
                executable: "/bin/true".into(),
                arguments: vec![],
                directory: "/tmp".into(),
                environment,
            },
            resolver,
            profile,
            "github",
            "github-mcp",
        );
        assert_eq!(
            factory
                .open(
                    &CallContext::new(
                        NonEmptyText::new("corr").unwrap(),
                        std::time::Instant::now() + Duration::from_secs(1)
                    ),
                    &Limits::default(),
                )
                .err()
                .unwrap(),
            Error::Configuration
        );
        let direct = StdioConfig {
            executable: "/bin/true".into(),
            arguments: vec![],
            directory: "/tmp".into(),
            environment: [("GH_TOKEN".to_owned(), "unscoped-secret".to_owned())]
                .into_iter()
                .collect(),
        };
        assert_eq!(
            direct
                .open(
                    &CallContext::new(
                        NonEmptyText::new("corr").unwrap(),
                        std::time::Instant::now() + Duration::from_secs(1),
                    ),
                    &Limits::default(),
                )
                .err()
                .unwrap(),
            Error::Configuration
        );
    }

    #[test]
    fn resolver_authentication_failure_is_typed_without_provider_text() {
        struct FailedAuth;
        impl ConnectorSecretResolver for FailedAuth {
            fn resolve(
                &self,
                _: &gateway_application::connectors::CredentialProfileRef,
            ) -> Result<ResolvedCredential, gateway_application::connectors::ConnectorFailureCode>
            {
                Err(gateway_application::connectors::ConnectorFailureCode::AuthenticationFailed)
            }
        }
        use gateway_application::connectors::CredentialProfileRef;
        let profile = CredentialProfileRef::new(NonEmptyText::new("profile.github").unwrap());
        let factory = CredentialedStdioFactory::new(
            StdioConfig {
                executable: "/bin/true".into(),
                arguments: vec![],
                directory: "/tmp".into(),
                environment: BTreeMap::new(),
            },
            std::sync::Arc::new(FailedAuth),
            profile,
            "github",
            "github-mcp",
        );
        assert_eq!(
            factory
                .open(
                    &CallContext::new(
                        NonEmptyText::new("corr").unwrap(),
                        std::time::Instant::now() + Duration::from_secs(1)
                    ),
                    &Limits::default(),
                )
                .err()
                .unwrap(),
            Error::Authentication
        );
    }
}
