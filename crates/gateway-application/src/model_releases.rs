//! CG-28 process-local release coordinator. Hosts persist manifests and the
//! append-only journal atomically before making a routing change visible.
use crate::offline_learning::{LearningError, QualifiedModel, digest, zero_digest};
use gateway_domain::{ContentDigest, ContextScopeId, UnixTimestamp, offline_learning::*};
use std::collections::BTreeMap;

/// Independent governance authority. Verify the actor and policy against the
/// exact scope, action, release digest, restoration and observation. JSON and
/// learning rewards cannot authenticate an approval or a canary result.
pub trait ModelReleaseAuthority {
    fn authorize(
        &self,
        scope: &ContextScopeId,
        event: &ModelReleaseEvent,
    ) -> Result<bool, LearningError>;
    /// Verify exact cohort/time/counts, candidate and unique underlying requests.
    fn verify_canary(
        &self,
        release: &ModelReleaseManifest,
        observation: &CanaryObservation,
    ) -> Result<bool, LearningError>;
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ModelReleaseState {
    Registered,
    Canary,
    Active,
    Superseded,
    RolledBack,
}
struct StoredRelease {
    manifest: ModelReleaseManifest,
    state: ModelReleaseState,
    successes: u64,
    failures: u64,
}
/// Explicit host API, never wired into runtime signal ingestion or inference.
/// This reference coordinator is process-local, not a durable deployment store.
pub struct ModelReleaseRegistry {
    scope: ContextScopeId,
    releases: BTreeMap<ModelVersion, StoredRelease>,
    active: Option<ModelVersion>,
    events: Vec<ModelReleaseEvent>,
}
impl ModelReleaseRegistry {
    pub fn new(scope: ContextScopeId) -> Self {
        Self {
            scope,
            releases: BTreeMap::new(),
            active: None,
            events: Vec::new(),
        }
    }
    pub fn active(&self) -> Option<&ModelVersion> {
        self.active.as_ref()
    }
    pub fn events(&self) -> &[ModelReleaseEvent] {
        &self.events
    }
    pub fn manifest(&self, model: &ModelVersion) -> Option<&ModelReleaseManifest> {
        self.releases.get(model).map(|r| &r.manifest)
    }
    pub fn state(&self, model: &ModelVersion) -> Option<ModelReleaseState> {
        self.releases.get(model).map(|r| r.state)
    }
    /// Admission requires a fresh opaque offline qualification and separate approval.
    pub fn register<A: ModelReleaseAuthority>(
        &mut self,
        authority: &A,
        qualified: QualifiedModel,
        canary: ModelCanary,
        decision: ModelReleaseDecision,
    ) -> Result<ModelVersion, LearningError> {
        if qualified.run.scope != self.scope {
            return Err(LearningError::ScopeMismatch);
        }
        if canary.cohorts.is_empty()
            || canary.starts_at < decision.at
            || canary.ends_at <= canary.starts_at
            || canary.max_requests == 0
            || canary.required_successes == 0
            || canary.required_successes > canary.max_requests
            || canary.max_failures >= canary.max_requests
            || decision.at < qualified.evaluation.evaluated_at
        {
            return Err(LearningError::InvalidRelease);
        }
        let model = qualified.run.candidate.clone();
        // An ID/version pair is immutable even if someone supplies another digest.
        if self
            .releases
            .keys()
            .any(|k| k.id == model.id && k.version == model.version)
        {
            return Err(LearningError::DuplicateIdentity);
        }
        let mut manifest = ModelReleaseManifest {
            schema_version: OFFLINE_LEARNING_VERSION,
            scope: self.scope.clone(),
            training: qualified.run,
            recipe: qualified.recipe,
            evaluation: qualified.evaluation,
            canary,
            predecessor: self.active.clone(),
            digest: zero_digest(),
        };
        manifest.digest = release_digest(&manifest);
        let event = ModelReleaseEvent {
            decision,
            action: ModelReleaseAction::Register,
            release_digest: manifest.digest.clone(),
            restored: None,
            observation: None,
        };
        self.check(authority, &event)?;
        self.releases.insert(
            model.clone(),
            StoredRelease {
                manifest,
                state: ModelReleaseState::Registered,
                successes: 0,
                failures: 0,
            },
        );
        self.events.push(event);
        Ok(model)
    }
    pub fn start_canary<A: ModelReleaseAuthority>(
        &mut self,
        authority: &A,
        model: &ModelVersion,
        decision: ModelReleaseDecision,
    ) -> Result<(), LearningError> {
        let release = self
            .releases
            .get(model)
            .ok_or(LearningError::InvalidRelease)?;
        if release.state != ModelReleaseState::Registered
            || release.manifest.predecessor != self.active
            || !within(&release.manifest.canary, decision.at)
        {
            return Err(LearningError::InvalidTransition);
        }
        let event = event(release, decision, ModelReleaseAction::StartCanary);
        self.check(authority, &event)?;
        self.releases.get_mut(model).expect("checked release").state = ModelReleaseState::Canary;
        self.events.push(event);
        Ok(())
    }
    pub fn observe_canary<A: ModelReleaseAuthority>(
        &mut self,
        authority: &A,
        model: &ModelVersion,
        observation: CanaryObservation,
        decision: ModelReleaseDecision,
    ) -> Result<(), LearningError> {
        let release = self
            .releases
            .get(model)
            .ok_or(LearningError::InvalidRelease)?;
        let boundary = &release.manifest.canary;
        let successes = release
            .successes
            .checked_add(observation.successes)
            .ok_or(LearningError::InvalidRun)?;
        let failures = release
            .failures
            .checked_add(observation.failures)
            .ok_or(LearningError::InvalidRun)?;
        let count = successes
            .checked_add(failures)
            .ok_or(LearningError::InvalidRun)?;
        if release.state != ModelReleaseState::Canary
            || !within(boundary, observation.observed_at)
            || observation.observed_at > decision.at
            || !boundary.cohorts.contains(&observation.cohort)
            || observation
                .successes
                .checked_add(observation.failures)
                .is_none_or(|n| n == 0)
            || count > boundary.max_requests
        {
            return Err(LearningError::InvalidTransition);
        }
        if self
            .events
            .iter()
            .filter_map(|e| e.observation.as_ref())
            .any(|o| o.id == observation.id || o.evidence == observation.evidence)
        {
            return Err(LearningError::DuplicateIdentity);
        }
        let mut event = event(release, decision, ModelReleaseAction::ObserveCanary);
        event.observation = Some(observation.clone());
        self.check(authority, &event)?;
        if !authority.verify_canary(&release.manifest, &observation)? {
            return Err(LearningError::Unverified);
        }
        let release = self.releases.get_mut(model).expect("checked release");
        release.successes = successes;
        release.failures = failures;
        self.events.push(event);
        Ok(())
    }
    pub fn activate<A: ModelReleaseAuthority>(
        &mut self,
        authority: &A,
        model: &ModelVersion,
        decision: ModelReleaseDecision,
    ) -> Result<(), LearningError> {
        let release = self
            .releases
            .get(model)
            .ok_or(LearningError::InvalidRelease)?;
        if release.state != ModelReleaseState::Canary
            || release.manifest.predecessor != self.active
            || release.successes < release.manifest.canary.required_successes
            || release.failures > release.manifest.canary.max_failures
            || decision.at < release.manifest.canary.starts_at
        {
            return Err(LearningError::InvalidTransition);
        }
        let event = event(release, decision, ModelReleaseAction::Activate);
        self.check(authority, &event)?;
        if let Some(previous) = &self.active {
            self.releases
                .get_mut(previous)
                .expect("active release")
                .state = ModelReleaseState::Superseded;
        }
        self.releases.get_mut(model).expect("checked release").state = ModelReleaseState::Active;
        self.active = Some(model.clone());
        self.events.push(event);
        Ok(())
    }
    /// Restores the exact previously active predecessor, retaining every manifest
    /// and observation. First-release rollback disables routing (active = None).
    pub fn rollback<A: ModelReleaseAuthority>(
        &mut self,
        authority: &A,
        model: &ModelVersion,
        decision: ModelReleaseDecision,
    ) -> Result<(), LearningError> {
        let release = self
            .releases
            .get(model)
            .ok_or(LearningError::InvalidRelease)?;
        let was_active = release.state == ModelReleaseState::Active;
        if !matches!(
            release.state,
            ModelReleaseState::Canary | ModelReleaseState::Active
        ) || (was_active && self.active.as_ref() != Some(model))
            || (!was_active && self.active != release.manifest.predecessor)
        {
            return Err(LearningError::InvalidTransition);
        }
        let predecessor = release.manifest.predecessor.clone();
        if was_active
            && predecessor
                .as_ref()
                .is_some_and(|p| self.state(p) != Some(ModelReleaseState::Superseded))
        {
            return Err(LearningError::InvalidTransition);
        }
        let mut event = event(release, decision, ModelReleaseAction::Rollback);
        event.restored = predecessor.clone();
        self.check(authority, &event)?;
        if was_active {
            if let Some(previous) = &predecessor {
                self.releases
                    .get_mut(previous)
                    .expect("checked predecessor")
                    .state = ModelReleaseState::Active;
            }
            self.active = predecessor;
        }
        self.releases.get_mut(model).expect("checked release").state =
            ModelReleaseState::RolledBack;
        self.events.push(event);
        Ok(())
    }
    fn check<A: ModelReleaseAuthority>(
        &self,
        authority: &A,
        event: &ModelReleaseEvent,
    ) -> Result<(), LearningError> {
        if self
            .events
            .iter()
            .any(|e| e.decision.id == event.decision.id)
            || self
                .events
                .last()
                .is_some_and(|e| e.decision.at > event.decision.at)
        {
            return Err(LearningError::DuplicateIdentity);
        }
        if !authority.authorize(&self.scope, event)? {
            return Err(LearningError::Unauthorized);
        }
        Ok(())
    }
}
fn within(boundary: &ModelCanary, at: UnixTimestamp) -> bool {
    at >= boundary.starts_at && at <= boundary.ends_at
}
fn event(
    release: &StoredRelease,
    decision: ModelReleaseDecision,
    action: ModelReleaseAction,
) -> ModelReleaseEvent {
    ModelReleaseEvent {
        decision,
        action,
        release_digest: release.manifest.digest.clone(),
        restored: None,
        observation: None,
    }
}
pub fn release_digest(manifest: &ModelReleaseManifest) -> ContentDigest {
    let mut copy = manifest.clone();
    copy.digest = zero_digest();
    digest(&copy)
}
