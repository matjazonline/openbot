//! Bounded final publication; transport calls remain with the existing delivery worker.
use super::*;
use crate::app_error::AppError;
use crate::application::workflow::message::{
    MessageArguments, MessagePlanner, MessagePublicationIdentity,
};
use crate::domain::monitoring::MonitoringService;
use std::sync::Arc;
use std::time::Duration;
use tokio_util::sync::CancellationToken;
use uuid::Uuid;

const PROCESS_LIMIT: Duration = Duration::from_secs(20);
const COMPOSE_LIMIT: Duration = Duration::from_secs(5);
const SHUTDOWN_RELEASE_LIMIT: Duration = Duration::from_secs(3);
const POLL: Duration = Duration::from_secs(1);
const CONCURRENCY: u8 = 2;

pub struct ChannelReplyPublisher<P> {
    store: P,
    planner: MessagePlanner,
    monitoring: Option<Arc<dyn MonitoringService>>,
}

impl<P: ReplyPublicationStore> ChannelReplyPublisher<P> {
    pub fn new(store: P, planner: MessagePlanner) -> Self {
        Self {
            store,
            planner,
            monitoring: None,
        }
    }

    pub fn with_monitoring(mut self, monitoring: Arc<dyn MonitoringService>) -> Self {
        self.monitoring = Some(monitoring);
        self
    }

    pub async fn publish_claim(
        &self,
        claim: ReplyPublicationClaim,
    ) -> AppResult<ReplyPublicationOutcome> {
        // Box this storage/rendering boundary rather than growing the worker's poll frame.
        let outcome = tokio::time::timeout(PROCESS_LIMIT, Box::pin(self.attempt(claim)))
            .await
            .map_err(|_| AppError::Timeout("Final reply publication timed out".into()))
            .and_then(|outcome| outcome);
        self.record(claim, &outcome);
        outcome
    }

    async fn attempt(&self, claim: ReplyPublicationClaim) -> AppResult<ReplyPublicationOutcome> {
        match self.publish_once(claim).await {
            Ok(outcome) => Ok(outcome),
            Err(error) => self.observe_or_defer(claim, error).await,
        }
    }

    async fn publish_once(
        &self,
        claim: ReplyPublicationClaim,
    ) -> AppResult<ReplyPublicationOutcome> {
        let draft = match self.store.load_reply_publication(claim).await? {
            ReplyPublicationLoad::Ready(draft) => *draft,
            ReplyPublicationLoad::Published(receipt) => {
                return Ok(ReplyPublicationOutcome::Published(receipt));
            }
            ReplyPublicationLoad::Failed(reason) => {
                return Ok(ReplyPublicationOutcome::Refused(reason));
            }
            ReplyPublicationLoad::Refused(reason) => {
                return self
                    .settle(claim, ReplyPublicationRetry::Terminal(reason))
                    .await;
            }
            ReplyPublicationLoad::Lost => return Ok(ReplyPublicationOutcome::Lost),
        };
        if draft.scope.company != claim.company
            || draft.scope.run != claim.run
            || draft.origin.proposal != claim.proposal
            || draft.origin.integrity != claim.integrity
        {
            return self
                .settle(
                    claim,
                    ReplyPublicationRetry::Terminal(ReplyPublicationFailure::InvalidProposal),
                )
                .await;
        }
        let arguments = MessageArguments::Reply {
            source: draft.source,
            content: draft.content,
        };
        let identity = MessagePublicationIdentity {
            company: claim.company,
            invocation: draft.invocation,
        };
        let plan = tokio::time::timeout(
            COMPOSE_LIMIT,
            Box::pin(self.planner.plan(identity, &arguments, &draft.routes)),
        )
        .await
        .map_err(|_| AppError::ServiceUnavailable("Reply composition timed out".into()))??;
        self.store.commit_reply_publication(claim, &plan).await
    }

    async fn observe_or_defer(
        &self,
        claim: ReplyPublicationClaim,
        error: AppError,
    ) -> AppResult<ReplyPublicationOutcome> {
        // A COMMIT acknowledgement can be lost. Observe the saved identity before retrying.
        match self.store.load_reply_publication(claim).await? {
            ReplyPublicationLoad::Published(receipt) => {
                Ok(ReplyPublicationOutcome::Published(receipt))
            }
            ReplyPublicationLoad::Failed(reason) => Ok(ReplyPublicationOutcome::Refused(reason)),
            ReplyPublicationLoad::Lost => Ok(ReplyPublicationOutcome::Lost),
            ReplyPublicationLoad::Refused(reason) => {
                self.settle(claim, ReplyPublicationRetry::Terminal(reason))
                    .await
            }
            ReplyPublicationLoad::Ready(_) => self.settle(claim, retry_for(&error)).await,
        }
    }

    async fn settle(
        &self,
        claim: ReplyPublicationClaim,
        retry: ReplyPublicationRetry,
    ) -> AppResult<ReplyPublicationOutcome> {
        if !self.store.release_reply(claim, retry).await? {
            return Ok(ReplyPublicationOutcome::Lost);
        }
        Ok(match retry {
            ReplyPublicationRetry::Retry => ReplyPublicationOutcome::Deferred,
            ReplyPublicationRetry::Terminal(reason) => ReplyPublicationOutcome::Refused(reason),
        })
    }

    pub async fn run(&self, shutdown: &CancellationToken) {
        let worker = ReplyPublicationWorker(Uuid::new_v4());
        let page = ReplyPublicationPage::try_from(CONCURRENCY).expect("static publication page");
        loop {
            let claimed = tokio::select! { biased;
                _ = shutdown.cancelled() => return,
                claimed = self.store.claim_replies(worker, page) => claimed,
            };
            let full = match claimed {
                Ok(claims) => {
                    let full = claims.len() == usize::from(CONCURRENCY);
                    let completed = {
                        let processing = futures::future::join_all(
                            claims.iter().map(|claim| self.publish_claim(*claim)),
                        );
                        tokio::pin!(processing);
                        tokio::select! { biased;
                            _ = shutdown.cancelled() => false,
                            _ = &mut processing => true,
                        }
                    };
                    if !completed {
                        self.release_on_shutdown(&claims).await;
                        return;
                    }
                    full
                }
                Err(_) => {
                    tracing::warn!(worker_id=%worker.0, code="claim_failed", "Final reply claim failed");
                    false
                }
            };
            if full {
                // Every claim moved out of immediate eligibility, including poison retries.
                tokio::task::yield_now().await;
                continue;
            }
            tokio::select! { biased;
                _ = shutdown.cancelled() => return,
                _ = tokio::time::sleep(POLL) => {},
            }
        }
    }

    async fn release_on_shutdown(&self, claims: &[ReplyPublicationClaim]) {
        let releases = futures::future::join_all(claims.iter().map(|claim| {
            self.store
                .release_reply(*claim, ReplyPublicationRetry::Retry)
        }));
        match tokio::time::timeout(SHUTDOWN_RELEASE_LIMIT, releases).await {
            Ok(results) => {
                let failed = results.iter().filter(|result| result.is_err()).count();
                if failed > 0 {
                    tracing::warn!(
                        failed,
                        code = "release_failed",
                        "Final reply claim release was unconfirmed"
                    );
                }
            }
            Err(_) => {
                tracing::warn!(
                    code = "release_unknown",
                    "Final reply claims will recover after expiry"
                );
            }
        }
    }

    fn record(&self, claim: ReplyPublicationClaim, outcome: &AppResult<ReplyPublicationOutcome>) {
        let status = match outcome {
            Ok(ReplyPublicationOutcome::Published(receipt)) => {
                tracing::info!(company_id=%claim.company.as_uuid(),run_id=%claim.run.as_uuid(),
                    proposal_id=%claim.proposal.as_uuid(),message_id=%receipt.message.as_uuid(),
                    delivery_id=?receipt.delivery, "Final reply published");
                "published"
            }
            Ok(ReplyPublicationOutcome::Refused(reason)) => {
                tracing::warn!(company_id=%claim.company.as_uuid(),run_id=%claim.run.as_uuid(),
                    proposal_id=%claim.proposal.as_uuid(),code=reason.as_str(), "Final reply refused");
                "failed"
            }
            Ok(ReplyPublicationOutcome::Deferred) => "retry",
            Ok(ReplyPublicationOutcome::Lost) => "lease_lost",
            Err(_) => {
                tracing::warn!(company_id=%claim.company.as_uuid(),run_id=%claim.run.as_uuid(),
                    proposal_id=%claim.proposal.as_uuid(),code="publication_unconfirmed", "Final reply publication was unconfirmed");
                "error"
            }
        };
        if let Some(monitoring) = &self.monitoring {
            monitoring.increment_counter(
                "workflow_channel_reply_publications_total",
                1,
                &[("outcome", status)],
            );
        }
    }
}

fn retry_for(error: &AppError) -> ReplyPublicationRetry {
    use ReplyPublicationFailure as Failure;
    let terminal = match error {
        AppError::KnowledgeRelease { .. } => Failure::ReleaseDenied,
        AppError::NotFound(_) | AppError::Forbidden(_) | AppError::InvalidCredentials => {
            Failure::AuthorityRevoked
        }
        AppError::BadRequest(_) | AppError::Internal(_) | AppError::WorkflowGuard(_) => {
            Failure::PublicationFailed
        }
        _ => return ReplyPublicationRetry::Retry,
    };
    ReplyPublicationRetry::Terminal(terminal)
}
