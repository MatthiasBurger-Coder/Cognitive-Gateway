use super::super::{BindingId, ClientOwnerId, PrincipalId, ProjectId, WorkspaceId};
use super::*;
use std::time::Duration;

#[test]
fn stale_or_expired_phase_authority_cannot_mint_or_refill_connector_allowance() {
    let amounts = SessionResourceAmounts {
        connector_calls: 2,
        ..Default::default()
    };
    let cost_unit = ReferenceId::new("credits").unwrap();
    let mut ledger = SessionResourceLedger::new(SessionResourceLimits {
        cost_unit: cost_unit.clone(),
        amounts,
    })
    .unwrap();
    let binding = ResourceReservationBinding {
        owner: OwnerBinding {
            principal: PrincipalId::new("operator").unwrap(),
            workspace: WorkspaceId::new("workspace").unwrap(),
            project: ProjectId::new("project").unwrap(),
            binding: BindingId::new("binding").unwrap(),
            client_owner: ClientOwnerId::new("client").unwrap(),
        },
        scope: ContextScopeId::new("project").unwrap(),
        session: SessionId::new("session").unwrap(),
        run: RunId::new("run").unwrap(),
        revision: Revision::new(2).unwrap(),
        step: PlanStepId::new("observe").unwrap(),
        dispatch: DispatchId::new("dispatch").unwrap(),
    };
    let reservation = ledger.reserve(binding, amounts).unwrap();
    let retained = ledger.clone();
    let fence = FenceToken::new(3).unwrap();
    let future = Instant::now() + Duration::from_secs(30);
    for (revision, deadline, error) in [
        (
            Revision::new(1).unwrap(),
            future,
            SessionError::StaleRevision,
        ),
        (
            Revision::new(2).unwrap(),
            Instant::now() - Duration::from_secs(1),
            SessionError::LimitExceeded,
        ),
    ] {
        assert_eq!(
            SessionPhaseResources::mint(
                &reservation,
                SessionResourcePhase::Preparation,
                cost_unit.clone(),
                revision,
                fence,
                deadline,
            )
            .unwrap_err(),
            error
        );
        assert_eq!(ledger, retained);
    }
    let mut phase = SessionPhaseResources::mint(
        &reservation,
        SessionResourcePhase::Preparation,
        cost_unit,
        Revision::new(2).unwrap(),
        fence,
        future,
    )
    .unwrap();
    let one = SessionResourceAmounts {
        connector_calls: 1,
        ..Default::default()
    };
    let child = phase.allocate(one).unwrap();
    assert_eq!(child.reservation_id(), reservation.id());
    assert_eq!(child.fence(), fence);
    assert_eq!(child.phase(), phase.phase());
    assert_eq!(phase.fence(), fence);
    assert_eq!(phase.remaining(), &one);
    drop(child);
    phase.deadline = Instant::now() - Duration::from_secs(1);
    assert_eq!(
        phase.allocate(one).unwrap_err(),
        SessionError::LimitExceeded
    );
    assert_eq!(phase.remaining(), &one);
    assert_eq!(ledger, retained);
    assert_eq!(ledger.used(), &amounts);
}
