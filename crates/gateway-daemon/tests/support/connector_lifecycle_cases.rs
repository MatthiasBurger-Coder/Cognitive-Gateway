use super::*;
use gateway_application::connector_admission::*;
use gateway_daemon::mcp_client::plugins::*;
fn binding() -> ConnectorScopeBinding {
    ConnectorScopeBinding {
        project: text("project-one"),
        scope: target().scope,
        session: Some(text("session-one")),
        request: Some(text("request-one")),
    }
}
fn profile() -> ConnectorCompatibilityProfile {
    ConnectorCompatibilityProfile {
        gateway: ConnectorVersion {
            major: 0,
            minor: 1,
            patch: 0,
        },
        transports: BTreeSet::from([ConnectorTransport::Stdio]),
    }
}
fn registration() -> ConnectorRegistration {
    ConnectorRegistration {
        binding: binding(),
        manifest: ConnectorManifest::from_json(include_bytes!(
            "../../../../tests/fixtures/connector-manifest-v1/valid-stdio.json"
        ))
        .unwrap(),
        enabled: true,
    }
}
fn registry() -> ConnectorRegistry {
    ConnectorRegistry::rebuild(vec![registration()], &profile()).unwrap()
}
fn plugin(mode: &'static str) -> PluginConfig {
    PluginConfig {
        host: HostConfig {
            target: target(),
            manifest: registration().manifest,
            profile: profile(),
            limits: Limits {
                call_timeout: Duration::from_millis(300),
                ..Limits::default()
            },
        },
        transport: Box::new(config(mode)),
        policy: Arc::new(Authority::default()),
        mapping: Box::new(Mapping { mode: "normal" }),
        observer: Arc::new(()),
    }
}
fn manager(mode: &'static str) -> PluginLifecycleManager {
    PluginLifecycleManager::new(registry(), binding(), vec![plugin(mode)]).unwrap()
}
fn ready_manager(m: &PluginLifecycleManager) -> ConnectorInventory {
    assert_eq!(
        m.lifecycle(&target(), ConnectorLifecycleCommand::Connect)
            .unwrap(),
        ConnectorLifecycleState::Ready
    );
    m.discover(&target()).unwrap()
}
#[test]
fn configured_plugins_are_scoped_reversible_and_rebuild_without_runtime_state() {
    let mut m = manager("normal");
    let original_index = m.index();
    assert_eq!(original_index, vec![registration()]);
    let inventory = ready_manager(&m);
    let mut r = request();
    r.inventory_revision = inventory.revision;
    m.invoke(&r).unwrap();
    m.read_resource(&r, &SourceId::new("one").unwrap()).unwrap();
    let mut other = r.clone();
    other.target.scope = ContextScopeId::new("other-project").unwrap();
    assert_eq!(
        m.invoke(&other).unwrap_err().code,
        ConnectorFailureCode::ScopeMismatch
    );
    assert_eq!(
        m.read_resource(&other, &SourceId::new("one").unwrap())
            .unwrap_err()
            .code,
        ConnectorFailureCode::ScopeMismatch
    );
    assert_eq!(
        m.discover(&other.target).unwrap_err().code,
        ConnectorFailureCode::ScopeMismatch
    );
    assert_eq!(
        m.remove(&other.target).unwrap_err().code,
        ConnectorFailureCode::ScopeMismatch
    );
    let mut unknown = target();
    unknown.connector = text("unadmitted");
    assert_eq!(
        m.discover(&unknown).unwrap_err().code,
        ConnectorFailureCode::NotFound
    );
    assert_eq!(
        m.remove(&unknown).unwrap_err().code,
        ConnectorFailureCode::NotFound
    );
    assert_eq!(
        m.lifecycle(&target(), ConnectorLifecycleCommand::Disable)
            .unwrap(),
        ConnectorLifecycleState::Disabled
    );
    assert_eq!(
        m.invoke(&r).unwrap_err().effect,
        ConnectorEffect::NotDispatched
    );
    assert!(m.discover(&target()).is_err());
    m.enable(&target()).unwrap();
    assert!(m.invoke(&r).is_err()); // no old inventory/session after enable
    assert_eq!(ready_manager(&m).revision, r.inventory_revision);
    m.invoke(&r).unwrap();
    // Invalid replacement is atomic and leaves current registration usable.
    assert_eq!(m.rebuild(registry(), vec![]), Err(Error::Configuration));
    m.invoke(&r).unwrap();
    m.rebuild(registry(), vec![plugin("normal")]).unwrap();
    assert_eq!(m.index(), original_index);
    assert!(m.invoke(&r).is_err());
    ready_manager(&m);
    m.invoke(&r).unwrap();
    m.remove(&target()).unwrap();
    assert!(m.invoke(&r).is_err());
    assert!(m.enable(&target()).is_err());
    // Canonical rebuild can explicitly re-add removed connector, never its cache.
    m.rebuild(registry(), vec![plugin("normal")]).unwrap();
    assert!(m.invoke(&r).is_err());
    ready_manager(&m);
    assert_eq!(
        m.lifecycle(&target(), ConnectorLifecycleCommand::Shutdown)
            .unwrap(),
        ConnectorLifecycleState::Disconnected
    );
    assert!(
        m.lifecycle(&target(), ConnectorLifecycleCommand::Connect)
            .is_err()
    );
    m.enable(&target()).unwrap();
    ready_manager(&m);
}
#[test]
fn plugin_configuration_rejects_missing_duplicate_and_mismatched_adapters() {
    assert!(matches!(
        PluginLifecycleManager::new(registry(), binding(), vec![]),
        Err(Error::Configuration)
    ));
    assert!(matches!(
        PluginLifecycleManager::new(
            registry(),
            binding(),
            vec![plugin("normal"), plugin("normal")]
        ),
        Err(Error::Configuration)
    ));
    let mut wrong = plugin("normal");
    wrong.host.target.scope = ContextScopeId::new("other").unwrap();
    assert!(matches!(
        PluginLifecycleManager::new(registry(), binding(), vec![wrong]),
        Err(Error::Configuration)
    ));
    let mut wrong = plugin("normal");
    let mut definition = serde_json::to_value(wrong.host.manifest.definition()).unwrap();
    definition["configuration_ref"] = "changed".into();
    wrong.host.manifest =
        ConnectorManifest::from_definition_json(&serde_json::to_vec(&definition).unwrap()).unwrap();
    assert!(matches!(
        PluginLifecycleManager::new(registry(), binding(), vec![wrong]),
        Err(Error::Configuration)
    ));
    let mut wrong = plugin("normal");
    wrong.host.limits.items = 0;
    assert!(matches!(
        PluginLifecycleManager::new(registry(), binding(), vec![wrong]),
        Err(Error::Configuration)
    ));
    let mut disabled = registration();
    disabled.enabled = false;
    let reg = ConnectorRegistry::rebuild(vec![disabled.clone()], &profile()).unwrap();
    assert!(PluginLifecycleManager::new(reg.clone(), binding(), vec![plugin("normal")]).is_err());
    let m = PluginLifecycleManager::new(reg, binding(), vec![]).unwrap();
    assert_eq!(m.index(), vec![disabled]);
    assert!(m.enable(&target()).is_err());
    let mut foreign = binding();
    foreign.project = text("foreign-project");
    assert!(
        PluginLifecycleManager::new(registry(), foreign.clone(), vec![plugin("normal")]).is_err()
    );
    let m = PluginLifecycleManager::new(registry(), foreign, vec![]).unwrap();
    assert!(m.index().is_empty());
}
#[test]
fn unavailable_connector_is_denied_until_explicit_recovery() {
    let mut m = manager("version");
    assert!(
        m.lifecycle(&target(), ConnectorLifecycleCommand::Connect)
            .is_err()
    );
    assert!(m.discover(&target()).is_err());
    m.lifecycle(&target(), ConnectorLifecycleCommand::Disable)
        .unwrap();
    m.rebuild(registry(), vec![plugin("normal")]).unwrap();
    ready_manager(&m);
}

#[test]
fn removal_cancels_in_flight_mutation_and_preserves_effect_uncertainty() {
    let events = Arc::new(Events::default());
    let mut config = plugin("slow");
    config.observer = events.clone();
    let m = Arc::new(PluginLifecycleManager::new(registry(), binding(), vec![config]).unwrap());
    let inventory = ready_manager(&m);
    let mut r = request();
    r.inventory_revision = inventory.revision;
    r.class = ConnectorOperationClass::Mutate;
    r.capability = CapabilityId::new("records.z-mutate").unwrap();
    let runner = m.clone();
    let thread = std::thread::spawn(move || runner.invoke(&r));
    let until = Instant::now() + Duration::from_secs(2);
    while !events
        .0
        .lock()
        .unwrap()
        .iter()
        .any(|e| e.kind == EventKind::Sent && e.correlation == request().correlation)
    {
        assert!(Instant::now() < until);
        std::thread::sleep(Duration::from_millis(1));
    }
    m.remove(&target()).unwrap();
    let failure = thread.join().unwrap().unwrap_err();
    assert_eq!(failure.effect, ConnectorEffect::Unknown);
    assert!(matches!(
        failure.code,
        ConnectorFailureCode::Cancelled | ConnectorFailureCode::PermissionDenied
    ));
    assert!(m.invoke(&request()).is_err());
}

#[test]
fn registry_revocation_is_polled_on_plain_host_transport_and_response_paths() {
    // Proves the guard even without the session-budget preflight wrapper.
    let events = Arc::new(Events::default());
    let admission = registry()
        .bind(&binding(), &target(), Arc::new(Authority::default()))
        .unwrap();
    let config = plugin("slow");
    let host = Arc::new(
        McpConnectorHost::new(
            config.host,
            config.transport,
            admission.clone(),
            config.mapping,
            events.clone(),
        )
        .unwrap(),
    );
    ready(&host);
    let runner = host.clone();
    let thread = std::thread::spawn(move || runner.invoke(&request()));
    let until = Instant::now() + Duration::from_secs(2);
    while !events
        .0
        .lock()
        .unwrap()
        .iter()
        .any(|e| e.kind == EventKind::Sent && e.correlation == request().correlation)
    {
        assert!(Instant::now() < until);
        std::thread::sleep(Duration::from_millis(1));
    }
    admission.revoke();
    let failure = thread.join().unwrap().unwrap_err();
    assert_eq!(failure.code, ConnectorFailureCode::PermissionDenied);
    assert_eq!(failure.effect, ConnectorEffect::NoMutation);
    assert!(host.discover(&target()).is_err());
    host.lifecycle(&target(), ConnectorLifecycleCommand::Shutdown)
        .unwrap();
}
