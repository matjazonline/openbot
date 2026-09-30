use super::*;
use crate::application::app_error::{AppError, AppResult};
use crate::application::workflow::{
    RelatedAssociation, WorkflowActor, WorkflowAuthorization, WorkflowOperation,
    binding::{ResourceDirectory, ResourceReadiness, ResourceStatus},
};
use crate::domain::workflow::{CompanyId, RuntimeResourceId, TypeName};
use async_trait::async_trait;
use std::sync::{
    Arc, Mutex,
    atomic::{AtomicUsize, Ordering},
};
use std::time::Duration;
use tokio_util::sync::CancellationToken;

struct SnapshotPort {
    snapshot: Mutex<Option<ReconciliationSnapshot>>,
    reads: Arc<AtomicUsize>,
}
#[async_trait]
impl ActionReconciliation for SnapshotPort {
    async fn association(&self, _: &ReconcileActionCommand) -> AppResult<RelatedAssociation> {
        self.reads.fetch_add(1, Ordering::SeqCst);
        Ok(RelatedAssociation::Company)
    }
    async fn snapshot(&self, _: &ReconcileActionCommand) -> AppResult<ReconciliationPreparation> {
        self.reads.fetch_add(1, Ordering::SeqCst);
        Ok(ReconciliationPreparation::Snapshot(Box::new(
            self.snapshot.lock().unwrap().take().unwrap(),
        )))
    }
    async fn settle(
        &self,
        _: &ReconcileActionCommand,
        _: &ReconciliationSnapshot,
        _: VerifiedEvidence,
    ) -> AppResult<ReconciliationResult> {
        panic!("unregistered source must never reach settlement")
    }
}
struct Authorizer(bool);
#[async_trait]
impl WorkflowAuthorization for Authorizer {
    async fn authorize(
        &self,
        _: CompanyId,
        _: WorkflowActor,
        _: RelatedAssociation,
        operation: WorkflowOperation,
    ) -> AppResult<()> {
        assert_eq!(operation, WorkflowOperation::Reconcile);
        if self.0 {
            Ok(())
        } else {
            Err(AppError::NotFound("workflow".into()))
        }
    }
}
struct Resources {
    expected_actor: WorkflowActor,
    fail: bool,
}
#[async_trait]
impl ResourceDirectory for Resources {
    async fn inspect(
        &self,
        company: CompanyId,
        actor: WorkflowActor,
        id: RuntimeResourceId,
    ) -> AppResult<Option<ResourceStatus>> {
        assert_eq!(
            actor, self.expected_actor,
            "reconciling actor owns this access check"
        );
        if self.fail {
            return Err(AppError::Database("directory outage".into()));
        }
        Ok(Some(ResourceStatus {
            id,
            company_id: company,
            kind: TypeName::parse("fixture").unwrap(),
            supported_contracts: [TypeName::parse("fixture.write").unwrap()].into(),
            authorized: true,
            readiness: ResourceReadiness::Ready,
        }))
    }
}

#[tokio::test]
async fn workflow_action_reconciliation_uninstalled_matching_source_never_settles() {
    let (command, snapshot, _matching_registration) = super::tests::fixture();
    let reads = Arc::new(AtomicUsize::new(0));
    // A syntactically matching reference cannot install a source through reconcile().
    let service = ActionReconciliationService::new(
        SnapshotPort {
            snapshot: Mutex::new(Some(snapshot)),
            reads: reads.clone(),
        },
        Authorizer(true),
        Resources {
            expected_actor: command.actor,
            fail: false,
        },
        None,
    );
    assert!(matches!(
        service
            .reconcile(&command, &CancellationToken::new(), Duration::from_secs(5))
            .await,
        Err(AppError::BadRequest(_))
    ));
    assert_eq!(reads.load(Ordering::SeqCst), 2);
}

#[tokio::test]
async fn workflow_action_reconciliation_denial_precedes_any_scope_read() {
    let (command, snapshot, _) = super::tests::fixture();
    let reads = Arc::new(AtomicUsize::new(0));
    let service = ActionReconciliationService::new(
        SnapshotPort {
            snapshot: Mutex::new(Some(snapshot)),
            reads: reads.clone(),
        },
        Authorizer(false),
        Resources {
            expected_actor: command.actor,
            fail: false,
        },
        None,
    );
    assert!(matches!(
        service
            .reconcile(&command, &CancellationToken::new(), Duration::from_secs(5))
            .await,
        Err(AppError::NotFound(_))
    ));
    assert_eq!(reads.load(Ordering::SeqCst), 0);
}

#[tokio::test]
async fn workflow_action_reconciliation_resource_error_preserves_operational_failure() {
    let (command, snapshot, _) = super::tests::fixture();
    let service = ActionReconciliationService::new(
        SnapshotPort {
            snapshot: Mutex::new(Some(snapshot)),
            reads: Arc::new(AtomicUsize::new(0)),
        },
        Authorizer(true),
        Resources {
            expected_actor: command.actor,
            fail: true,
        },
        None,
    );
    assert!(
        matches!(service.reconcile(&command, &CancellationToken::new(), Duration::from_secs(5))
        .await, Err(AppError::Database(message)) if message == "directory outage")
    );
}
