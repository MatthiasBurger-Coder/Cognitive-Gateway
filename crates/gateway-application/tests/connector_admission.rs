use gateway_application::{connector_admission::*, connector_manifest::*, connectors::*};
use gateway_domain::*;
use std::{
    collections::BTreeSet,
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, Ordering},
    },
};
fn text(s: &str) -> NonEmptyText {
    NonEmptyText::new(s).unwrap()
}
fn registration() -> ConnectorRegistration {
    ConnectorRegistration {
        binding: ConnectorScopeBinding {
            project: text("project-one"),
            scope: ContextScopeId::new("scope-one").unwrap(),
            session: Some(text("session-one")),
            request: Some(text("request-one")),
        },
        manifest: ConnectorManifest::from_json(include_bytes!(
            "../../../tests/fixtures/connector-manifest-v1/valid-stdio.json"
        ))
        .unwrap(),
        enabled: true,
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
fn target(r: &ConnectorRegistration) -> ConnectorTarget {
    ConnectorTarget {
        scope: r.binding.scope.clone(),
        connector: text(r.manifest.connector_id()),
    }
}
fn changed(r: &ConnectorRegistration, field: &str, value: serde_json::Value) -> ConnectorManifest {
    let mut d = serde_json::to_value(r.manifest.definition()).unwrap();
    d[field] = value;
    ConnectorManifest::from_definition_json(&serde_json::to_vec(&d).unwrap()).unwrap()
}
#[test]
fn canonical_rebuild_is_order_independent_and_never_inherits_authority() {
    let a = registration();
    let mut b = a.clone();
    b.binding.scope = ContextScopeId::new("scope-two").unwrap();
    b.binding.project = text("project-two");
    let first = ConnectorRegistry::rebuild(vec![a.clone(), b.clone()], &profile()).unwrap();
    assert_eq!(
        first,
        ConnectorRegistry::rebuild(vec![b.clone(), a.clone()], &profile()).unwrap()
    );
    assert_eq!(first.registrations().count(), 2);
    assert!(first.admitted(&a.binding, &target(&a)).is_ok());
    assert_eq!(
        first.admitted(&a.binding, &target(&b)),
        Err(AdmissionError::ScopeMismatch)
    );
    for field in ["project", "session", "request"] {
        let mut binding = a.binding.clone();
        match field {
            "project" => binding.project = text("forged"),
            "session" => binding.session = None,
            _ => binding.request = Some(text("other")),
        }
        assert_eq!(
            first.admitted(&binding, &target(&a)),
            Err(AdmissionError::ScopeMismatch)
        );
        let mut collision = a.clone();
        collision.binding = binding;
        assert_eq!(
            ConnectorRegistry::rebuild(vec![a.clone(), collision], &profile()),
            Err(AdmissionError::ScopeMismatch)
        );
    }
    let mut unknown = target(&a);
    unknown.connector = text("unknown");
    assert_eq!(
        first.admitted(&a.binding, &unknown),
        Err(AdmissionError::NotAdmitted)
    );
    let mut disabled = a.clone();
    disabled.enabled = false;
    assert_eq!(
        ConnectorRegistry::rebuild(vec![disabled.clone()], &profile())
            .unwrap()
            .admitted(&a.binding, &target(&a)),
        Err(AdmissionError::NotAdmitted)
    );
    disabled.manifest = changed(&a, "lifecycle", "disabled".into());
    assert!(ConnectorRegistry::rebuild(vec![disabled.clone()], &profile()).is_ok());
    disabled.enabled = true;
    assert_eq!(
        ConnectorRegistry::rebuild(vec![disabled], &profile()),
        Err(AdmissionError::Incompatible)
    );
}
#[test]
fn duplicates_conflicts_and_incompatible_manifests_fail_atomically() {
    let a = registration();
    assert_eq!(
        ConnectorRegistry::rebuild(vec![a.clone(), a.clone()], &profile()),
        Err(AdmissionError::DuplicateIdentity)
    );
    let mut b = a.clone();
    b.enabled = false;
    assert_eq!(
        ConnectorRegistry::rebuild(vec![a.clone(), b.clone()], &profile()),
        Err(AdmissionError::ConflictingIdentity)
    );
    b.manifest = changed(&a, "connector_id", "another.connector".into());
    assert_eq!(
        ConnectorRegistry::rebuild(vec![a.clone(), b], &profile()),
        Err(AdmissionError::ConflictingIdentity)
    );
    let mut bad = profile();
    bad.gateway.major = 99;
    assert_eq!(
        ConnectorRegistry::rebuild(vec![a.clone()], &bad),
        Err(AdmissionError::Incompatible)
    );
    bad = profile();
    bad.transports.clear();
    assert_eq!(
        ConnectorRegistry::rebuild(vec![a], &bad),
        Err(AdmissionError::Incompatible)
    );
    assert_eq!(
        ConnectorRegistry::rebuild(vec![], &profile())
            .unwrap()
            .registrations()
            .count(),
        0
    );
}
struct Policy {
    deny: AtomicBool,
    callback: Mutex<Option<Arc<ScopedConnectorAdmission>>>,
}
impl Policy {
    fn check(&self, target: &ConnectorTarget) -> Result<(), ConnectorFailure> {
        if let Some(lease) = self.callback.lock().unwrap().take() {
            lease.revoke();
        }
        if self.deny.load(Ordering::Acquire) {
            Err(ConnectorFailure {
                code: ConnectorFailureCode::PermissionDenied,
                effect: ConnectorEffect::NotDispatched,
                correlation: target.connector.clone(),
            })
        } else {
            Ok(())
        }
    }
}
impl ConnectorAdmissionPort for Policy {
    fn admit_target(&self, t: &ConnectorTarget) -> Result<(), ConnectorFailure> {
        self.check(t)
    }
    fn authorize(
        &self,
        r: &ConnectorRequest,
        _: Option<&SourceId>,
    ) -> Result<ConnectorDispatch, ConnectorFailure> {
        self.check(&r.target)?;
        Ok(ConnectorDispatch {
            request: r.clone(),
            policy: PolicyId::new("policy").unwrap(),
            policy_revision: context_content_digest(b"policy"),
            consent_receipt: None,
            budget_reservation: text("reservation"),
            deadline: UnixTimestamp::new(9999999999),
        })
    }
    fn revalidate(
        &self,
        d: &ConnectorDispatch,
        _: Option<&SourceId>,
    ) -> Result<(), ConnectorFailure> {
        self.check(&d.request.target)
    }
    fn validate_current_dispatch(
        &self,
        d: &ConnectorDispatch,
        _: Option<&SourceId>,
    ) -> Result<(), ConnectorFailure> {
        self.check(&d.request.target)
    }
}
#[test]
fn admission_preserves_policy_and_revokes_all_bound_operation_paths() {
    let r = registration();
    let t = target(&r);
    let registry = ConnectorRegistry::rebuild(vec![r.clone()], &profile()).unwrap();
    let p = Arc::new(Policy {
        deny: AtomicBool::new(false),
        callback: Mutex::new(None),
    });
    let bind = || registry.bind(&r.binding, &t, p.clone()).unwrap();
    let lease = bind();
    let request = ConnectorRequest {
        target: t.clone(),
        capability: CapabilityId::new("records.read").unwrap(),
        class: ConnectorOperationClass::Read,
        inventory_revision: context_content_digest(b"inventory"),
        arguments: Default::default(),
        task: TaskId::new("task").unwrap(),
        step: PlanStepId::new("step").unwrap(),
        execution: text("execution"),
        correlation: text("call"),
    };
    let dispatch = lease.authorize(&request, None).unwrap();
    lease.admit_target(&t).unwrap();
    lease.revalidate(&dispatch, None).unwrap();
    lease.validate_current_dispatch(&dispatch, None).unwrap();
    p.deny.store(true, Ordering::Release);
    assert!(lease.admit_target(&t).is_err());
    assert!(lease.authorize(&request, None).is_err());
    assert!(lease.revalidate(&dispatch, None).is_err());
    assert!(lease.validate_current_dispatch(&dispatch, None).is_err());
    p.deny.store(false, Ordering::Release);
    lease.disable();
    assert!(lease.admit_target(&t).is_err());
    assert!(lease.authorize(&request, None).is_err());
    assert!(lease.revalidate(&dispatch, None).is_err());
    assert!(lease.validate_current_dispatch(&dispatch, None).is_err());
    lease.enable().unwrap();
    lease.admit_target(&t).unwrap();
    let mut wrong = t.clone();
    wrong.scope = ContextScopeId::new("wrong").unwrap();
    assert_eq!(
        lease.admit_target(&wrong).unwrap_err().code,
        ConnectorFailureCode::ScopeMismatch
    );
    lease.revoke();
    assert_eq!(lease.enable(), Err(AdmissionError::NotAdmitted));
    assert!(lease.admit_target(&t).is_err());
    // Revocation inside any policy callback must also prevent release.
    for stage in 0..4 {
        let lease = bind();
        *p.callback.lock().unwrap() = Some(lease.clone());
        let denied = match stage {
            0 => lease.admit_target(&t),
            1 => lease.authorize(&request, None).map(|_| ()),
            2 => lease.revalidate(&dispatch, None),
            _ => lease.validate_current_dispatch(&dispatch, None),
        };
        assert_eq!(
            denied.unwrap_err().code,
            ConnectorFailureCode::PermissionDenied
        );
    }
    let mut wrong_binding = r.binding.clone();
    wrong_binding.request = None;
    assert!(registry.bind(&wrong_binding, &t, p).is_err());
}
