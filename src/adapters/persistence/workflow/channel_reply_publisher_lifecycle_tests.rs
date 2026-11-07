use super::*;
use crate::application::ports::integration::*;
use crate::entities::transport::{BindingAuditEvent, ChannelBinding, ChannelBindingId};
use std::sync::atomic::{AtomicBool, Ordering};

struct DropSignal(Arc<AtomicBool>);
impl Drop for DropSignal {
    fn drop(&mut self) {
        self.0.store(true, Ordering::SeqCst);
    }
}
struct GatedBindings {
    p: PostgresPersistence,
    entered: Arc<Notify>,
    dropped: Arc<AtomicBool>,
}
#[async_trait::async_trait]
impl ChannelBindingPersistence for GatedBindings {
    async fn active_bindings_for_channel(
        &self,
        _: Uuid,
        _: Uuid,
    ) -> AppResult<Vec<ChannelBinding>> {
        let _drop = DropSignal(self.dropped.clone());
        self.entered.notify_one();
        std::future::pending().await
    }
    async fn create_binding(&self, write: BindingWrite) -> AppResult<ChannelBinding> {
        self.p.create_binding(write).await
    }
    async fn find_active_binding_by_endpoint(
        &self,
        endpoint: &InboundEndpoint,
    ) -> AppResult<Option<ChannelBinding>> {
        self.p.find_active_binding_by_endpoint(endpoint).await
    }
    async fn list_bindings_for_company(&self, company: Uuid) -> AppResult<Vec<ChannelBinding>> {
        self.p.list_bindings_for_company(company).await
    }
    async fn get_binding(
        &self,
        company: Uuid,
        binding: ChannelBindingId,
    ) -> AppResult<Option<ChannelBinding>> {
        self.p.get_binding(company, binding).await
    }
    async fn set_binding_status(&self, change: BindingStatusChange) -> AppResult<ChannelBinding> {
        self.p.set_binding_status(change).await
    }
    async fn list_binding_audit_events(
        &self,
        company: Uuid,
        binding: ChannelBindingId,
        limit: i64,
    ) -> AppResult<Vec<BindingAuditEvent>> {
        self.p
            .list_binding_audit_events(company, binding, limit)
            .await
    }
}

#[tokio::test]
async fn channel_reply_recovery_shutdown_cancels_actual_composition_and_releases_claim() {
    let f = FinalFixture::new("data.map", false).await;
    f.advance().await;
    let entered = Arc::new(Notify::new());
    let dropped = Arc::new(AtomicBool::new(false));
    let renderers = crate::transport::TransportRenderers::new()
        .register(Arc::new(
            crate::adapters::protocols::email::EmailRenderer::new("agents.test"),
        ))
        .unwrap();
    let composer = crate::transport::DeliveryComposer::new(
        Arc::new(renderers),
        Arc::new(GatedBindings {
            p: f.p.clone(),
            entered: entered.clone(),
            dropped: dropped.clone(),
        }),
    );
    let worker = ChannelReplyPublisher::new(
        f.p.clone(),
        MessagePlanner::new(composer, "agents.test".into()),
    );
    let shutdown = CancellationToken::new();
    let token = shutdown.clone();
    let owner = tokio::spawn(async move { worker.run(&token).await });
    tokio::time::timeout(Duration::from_secs(3), entered.notified())
        .await
        .unwrap();
    shutdown.cancel();
    tokio::time::timeout(Duration::from_secs(4), owner)
        .await
        .unwrap()
        .unwrap();
    assert!(dropped.load(Ordering::SeqCst));
    assert_eq!(f.state().await["decision"], "pending");
    assert_eq!(outbound_count(&f).await, 0);
}

struct HeldClaim {
    p: PostgresPersistence,
    entered: Arc<Notify>,
    dropped: Arc<AtomicBool>,
}
#[async_trait::async_trait]
impl ReplyPublications for HeldClaim {
    async fn claim_replies(
        &self,
        worker: ReplyPublicationWorker,
        page: ReplyPublicationPage,
    ) -> AppResult<Vec<ReplyPublicationClaim>> {
        let _claimed = self.p.claim_replies(worker, page).await?;
        let _drop = DropSignal(self.dropped.clone());
        self.entered.notify_one();
        std::future::pending().await
    }
    async fn release_reply(
        &self,
        claim: ReplyPublicationClaim,
        retry: ReplyPublicationRetry,
    ) -> AppResult<bool> {
        self.p.release_reply(claim, retry).await
    }
}
#[async_trait::async_trait]
impl ReplyPublicationStore for HeldClaim {
    async fn load_reply_publication(
        &self,
        claim: ReplyPublicationClaim,
    ) -> AppResult<ReplyPublicationLoad> {
        self.p.load_reply_publication(claim).await
    }
    async fn commit_reply_publication(
        &self,
        claim: ReplyPublicationClaim,
        plan: &crate::application::workflow::message::MessagePlan,
    ) -> AppResult<ReplyPublicationOutcome> {
        self.p.commit_reply_publication(claim, plan).await
    }
}

#[tokio::test]
async fn channel_reply_recovery_shutdown_during_unacknowledged_claim_retains_real_lease() {
    let f = FinalFixture::new("data.map", false).await;
    f.advance().await;
    let entered = Arc::new(Notify::new());
    let dropped = Arc::new(AtomicBool::new(false));
    let worker = ChannelReplyPublisher::new(
        HeldClaim {
            p: f.p.clone(),
            entered: entered.clone(),
            dropped: dropped.clone(),
        },
        MessagePlanner::new(approval_test_support::composer(&f.p), "agents.test".into()),
    );
    let shutdown = CancellationToken::new();
    let token = shutdown.clone();
    let owner = tokio::spawn(async move { worker.run(&token).await });
    tokio::time::timeout(Duration::from_secs(3), entered.notified())
        .await
        .unwrap();
    shutdown.cancel();
    tokio::time::timeout(Duration::from_secs(4), owner)
        .await
        .unwrap()
        .unwrap();
    assert!(dropped.load(Ordering::SeqCst));
    assert_eq!(
        f.state().await["decision"],
        "claimed",
        "unacknowledged ownership is recovered through its real lease"
    );
    assert_eq!(outbound_count(&f).await, 0);
}

#[tokio::test]
async fn channel_reply_recovery_shutdown_cancels_actual_publication_transaction() {
    let f = FinalFixture::new("data.map", false).await;
    f.advance().await;
    let before = outbound_count(&f).await;
    let mut blocker = f.p.pool().begin().await.unwrap();
    sqlx::query("SELECT id FROM channel_bindings WHERE company_id=$1 AND channel_id=$2 FOR UPDATE")
        .bind(f.scope.company.as_uuid())
        .bind(f.s.channel)
        .fetch_all(&mut *blocker)
        .await
        .unwrap();
    let worker = publisher(&f);
    let shutdown = CancellationToken::new();
    let token = shutdown.clone();
    let owner = tokio::spawn(async move { worker.run(&token).await });
    crate::adapters::persistence::test_support::wait_until_a_backend_is_blocked(f.p.pool()).await;
    shutdown.cancel();
    blocker.rollback().await.unwrap();
    tokio::time::timeout(Duration::from_secs(4), owner)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(f.state().await["decision"], "pending");
    assert_eq!(outbound_count(&f).await, before);
}
