//! Pure checkpoint preview determines whether this operation can create a token review.
use super::*;
use crate::application::workflow::activation::ActivationRequest;

#[derive(Clone, Copy)]
pub(super) enum Purpose<'a> {
    Reserve(&'a WorkflowModelRequest),
    Settle(&'a SavedModelTurn),
}

pub(super) struct ModelAuthority {
    pub(super) actor: Option<WorkflowActor>,
    pub(super) reviewers_available: bool,
}

pub(super) enum Preparation {
    Stale,
    Current(ModelAuthority),
}

#[derive(PartialEq, Eq)]
struct ModelHints {
    checkpoint: Saved,
    reviewers: Option<token_budget::TokenReviewerAccounts>,
}

impl ModelHints {
    fn users(&self) -> impl Iterator<Item = Uuid> + '_ {
        self.reviewers
            .iter()
            .flat_map(|reviewers| reviewers.users())
    }
}

async fn hints_on(
    db: &mut PgConnection,
    write: WorkflowCheckpointWrite,
    purpose: Purpose<'_>,
    notices: Option<&crate::application::workflow::decisions::DecisionNotices>,
) -> AppResult<ModelHints> {
    let checkpoint = observe_on(db, write.fence).await?.ok_or_else(missing)?;
    let current = decode(checkpoint.id, checkpoint.checkpoint.clone())?;
    let can_park = match purpose {
        Purpose::Reserve(request) => {
            if admission_replay(&checkpoint, &current, write, &request.reservation)?.is_some()
                || notices.is_none()
            {
                false
            } else {
                current.apply(Mutation::Reserve(request.reservation.clone()))?;
                let root = token_budget::observe_scope(db, write.fence.scope).await?;
                root.refusal.is_none()
                    && !root.model_exhausted
                    && root.requires_approval(review_request_tokens(&root, request, notices)?)?
            }
        }
        Purpose::Settle(turn) => {
            if settlement_replay(&checkpoint, &current, write, turn)?.is_some()
                || notices.is_none()
                || current.apply(Mutation::Model(turn.clone()))?.0.state != RunState::Active
            {
                false
            } else {
                settlement_requires_review(db, write.fence.scope, turn).await?
            }
        }
    };
    let reviewers = if can_park {
        Some(token_budget::reviewer_accounts_on(db, write.fence.scope).await?)
    } else {
        None
    };
    Ok(ModelHints {
        checkpoint,
        reviewers,
    })
}

async fn settlement_requires_review(
    db: &mut PgConnection,
    scope: ActivationRequest,
    turn: &SavedModelTurn,
) -> AppResult<bool> {
    let mut root = token_budget::observe_scope(db, scope).await?;
    if root.refusal.is_some() || root.original.is_none() {
        return Ok(false);
    }
    let reserved: Option<i64> = sqlx::query_scalar(
        "SELECT call.reserved_input+call.reserved_output FROM model_usage_calls AS call \
         JOIN workflow_run_budgets AS link USING(company_id,root_run_id) \
         WHERE link.company_id=$1 AND link.run_id=$2 AND call.request_id=$3 AND call.settled_at IS NULL",
    )
    .bind(scope.company.as_uuid())
    .bind(scope.run.as_uuid())
    .bind(turn.request_id.0)
    .fetch_optional(db)
    .await?;
    // A concurrent settlement is reclassified by the checkpoint recheck; it cannot create a
    // second review. Replace just this request's reservation, as the actual ledger writer does.
    let Some(reserved) = reserved else {
        return Ok(false);
    };
    root.usage.unconfirmed = root
        .usage
        .unconfirmed
        .checked_sub(reserved)
        .ok_or_else(invalid)?;
    root.requires_approval(
        turn.input_tokens
            .checked_add(turn.output_tokens)
            .ok_or_else(invalid)?,
    )
}

pub(super) async fn prepare_on(
    db: &mut PgConnection,
    write: WorkflowCheckpointWrite,
    purpose: Purpose<'_>,
    notices: Option<&crate::application::workflow::decisions::DecisionNotices>,
) -> AppResult<Preparation> {
    let scope = ActionScope {
        company: write.fence.scope.company,
        run: write.fence.scope.run,
        execution: write.fence.scope.execution,
    };
    let Some(actors) = action_authority::actor_hints_on(db, scope, None).await? else {
        return Ok(Preparation::Current(ModelAuthority {
            actor: None,
            reviewers_available: false,
        }));
    };
    let hints = hints_on(db, write, purpose, notices).await?;
    let mut users = actors.users().chain(hints.users()).collect();
    let locked = reviewer_accounts::lock_users_on(db, &mut users).await?;
    if locked != users {
        return Ok(Preparation::Stale);
    }
    let actor = actors.actor()?;
    match authority::authorize_company(db, scope.company, actor).await {
        Ok(_) => {}
        Err(AppError::NotFound(_)) => {
            return Ok(Preparation::Current(ModelAuthority {
                actor: None,
                reviewers_available: false,
            }));
        }
        Err(error) => return Err(error),
    }
    // Company precedes root ownership. Keep accounting stable through the exact applicability
    // recheck and the later model write, so an unneeded reviewer never becomes a late lock.
    token_budget::lock_on(db, write.fence).await?;
    if action_authority::actor_hints_on(db, scope, None)
        .await?
        .as_ref()
        != Some(&actors)
        || hints_on(db, write, purpose, notices).await? != hints
    {
        return Ok(Preparation::Stale);
    }
    let reviewers_available = match &hints.reviewers {
        Some(reviewers) => reviewers.available_on(db, scope.company).await?,
        None => true,
    };
    Ok(Preparation::Current(ModelAuthority {
        actor: Some(actor),
        reviewers_available,
    }))
}

pub(super) fn changed() -> AppError {
    AppError::Conflict(
        "Model reviewer authority changed during account locking; retry the operation".into(),
    )
}
