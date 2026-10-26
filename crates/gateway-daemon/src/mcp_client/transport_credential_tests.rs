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
    let factory = CredentialedStdioFactory::new(config, resolver, profile, "github", "github-mcp");
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

#[test]
fn actual_connection_rotation_replaces_active_scope_and_secret_detection_without_profile_fallback()
{
    use gateway_application::connectors::CredentialProfileRef;
    use std::{
        sync::Arc,
        time::{Duration, Instant},
    };
    let profile = CredentialProfileRef::new(NonEmptyText::new("profile.rotation").unwrap());
    let resolver = Arc::new(ReloadableCredentialResolver::default());
    let factory = CredentialedStdioFactory::new(
        StdioConfig {
            executable: "/bin/cat".into(),
            arguments: vec![],
            directory: "/tmp".into(),
            environment: BTreeMap::new(),
        },
        resolver.clone(),
        profile.clone(),
        "github",
        "github-mcp",
    );
    let context = CallContext::new(
        NonEmptyText::new("credential-rotation").unwrap(),
        Instant::now() + Duration::from_secs(2),
    );
    resolver.replace(
        &profile,
        credential(&[("GITHUB_TOKEN", "old-private-canary")]),
    );
    let mut first = factory.open(&context, &Limits::default()).unwrap();
    assert!(factory.contains_secret("old-private-canary"));
    assert_eq!(
        factory.permits_tool("github", "github-mcp", "issues.read"),
        Ok(())
    );
    // Rotation applies to a new connection; it never silently rewrites the
    // credential scope or secret detection of an already open connection.
    resolver.replace(
        &profile,
        credential(&[("GITHUB_TOKEN", "new-private-canary")]),
    );
    assert!(factory.contains_secret("old-private-canary"));
    assert!(!factory.contains_secret("new-private-canary"));
    first.close();
    factory.connection_closed();
    assert!(!factory.contains_secret("old-private-canary"));
    assert_eq!(
        factory.permits_tool("github", "github-mcp", "issues.read"),
        Err(Error::CredentialScopeDenied)
    );
    let mut second = factory.open(&context, &Limits::default()).unwrap();
    assert!(factory.contains_secret("new-private-canary"));
    assert!(!factory.contains_secret("old-private-canary"));
    second.close();
    let mut foreign = credential(&[("GITHUB_TOKEN", "foreign-private-canary")]);
    foreign.scope.server = NonEmptyText::new("foreign-mcp").unwrap();
    resolver.replace(&profile, foreign);
    assert!(matches!(
        factory.open(&context, &Limits::default()),
        Err(Error::CredentialScopeDenied)
    ));
    assert!(!factory.contains_secret("new-private-canary"));
    assert!(!factory.contains_secret("foreign-private-canary"));
    assert_eq!(
        factory.permits_tool("github", "github-mcp", "issues.read"),
        Err(Error::CredentialScopeDenied)
    );
}
