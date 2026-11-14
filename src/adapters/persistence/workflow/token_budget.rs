//! The root usage row serializes token facts without locking parent workflow runs.
use super::*;
use crate::application::workflow::activation::ActivationRequest;
use crate::application::workflow::lease::WorkflowFence;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

#[derive(Debug, Clone, Serialize, Deserialize, sqlx::FromRow)]
pub(super) struct TokenUsage {
    pub reported: i64,
    pub estimated: i64,
    pub mixed: i64,
    pub unconfirmed: i64,
}
impl TokenUsage {
    pub fn total(&self) -> AppResult<u64> {
        [self.reported, self.estimated, self.mixed, self.unconfirmed]
            .into_iter()
            .try_fold(0_u64, |sum, amount| {
                sum.checked_add(u64::try_from(amount).map_err(|_| invalid())?)
                    .ok_or_else(invalid)
            })
    }
}

pub(super) struct RootTokens {
    pub original: Option<u64>,
    pub allowance: u64,
    pub refusal: Option<crate::domain::workflow::TokenRefusal>,
    pub usage: TokenUsage,
    pub model_exhausted: bool,
}
impl RootTokens {
    pub(super) fn progression(self) -> AppResult<Progression> {
        Ok(if let Some(reason) = self.refusal {
            Progression::Refused(reason)
        } else if self.requires_approval(0)? {
            Progression::AwaitingAllowance
        } else {
            Progression::Open
        })
    }

    pub fn requires_approval(&self, additional: u64) -> AppResult<bool> {
        Ok(self.original.is_some()
            && self
                .usage
                .total()?
                .checked_add(additional)
                .ok_or_else(invalid)?
                > self.allowance)
    }
    pub fn proposal(
        &self,
        request: Option<&crate::application::workflow::checkpoints::WorkflowModelRequest>,
    ) -> AppResult<Value> {
        let original = self.original.ok_or_else(invalid)?;
        let needed = request.map_or(Ok(0), |request| {
            request
                .reservation
                .input_tokens
                .checked_add(request.reservation.output_tokens)
                .ok_or_else(invalid)
        })?;
        let proposed = self
            .allowance
            .checked_add(original)
            .ok_or_else(invalid)?
            .max(
                self.usage
                    .total()?
                    .checked_add(needed)
                    .ok_or_else(invalid)?,
            );
        if proposed > i64::MAX as u64 {
            return Err(invalid());
        }
        Ok(
            json!({"original_allowance":original,"old_allowance":self.allowance,"proposed_allowance":proposed,
            "usage":self.usage,"request":request.map(|request| request.saved())}),
        )
    }
}

#[derive(sqlx::FromRow)]
struct RootRow {
    root_run_id: Uuid,
    original: Option<i64>,
    allowance: i64,
    token_refusal: Option<String>,
    model_exhausted: bool,
}

pub(super) fn lock_on(
    db: &mut PgConnection,
    fence: WorkflowFence,
) -> impl std::future::Future<Output = AppResult<RootTokens>> + '_ {
    lock_scope(db, fence.scope)
}

pub(super) fn lock_scope(
    db: &mut PgConnection,
    scope: ActivationRequest,
) -> impl std::future::Future<Output = AppResult<RootTokens>> + '_ {
    read_scope(db, scope, RootRead::Lock)
}

pub(super) fn observe_scope(
    db: &mut PgConnection,
    scope: ActivationRequest,
) -> impl std::future::Future<Output = AppResult<RootTokens>> + '_ {
    read_scope(db, scope, RootRead::Observe)
}

enum RootRead {
    Observe,
    Lock,
}

async fn read_scope(
    db: &mut PgConnection,
    scope: ActivationRequest,
    purpose: RootRead,
) -> AppResult<RootTokens> {
    const ROOT: &str = "SELECT usage.root_run_id,(budget.token_policy->>'max_tokens')::bigint AS original,
        GREATEST(usage.authorized_tokens,COALESCE((budget.token_policy->>'max_tokens')::bigint,0)) AS allowance,usage.token_refusal,
        (usage.model_calls>=budget.model_calls) AS model_exhausted
        FROM workflow_run_budgets AS link JOIN workflow_root_budget_usage AS usage USING(company_id,root_run_id)
        JOIN workflow_root_budgets AS budget USING(company_id,root_run_id)
        WHERE link.company_id=$1 AND link.run_id=$2";
    let query = match purpose {
        RootRead::Observe => ROOT.to_owned(),
        RootRead::Lock => format!("{ROOT} FOR UPDATE OF usage"),
    };
    let root: RootRow = sqlx::query_as(&query)
        .bind(scope.company.as_uuid())
        .bind(scope.run.as_uuid())
        .fetch_one(&mut *db)
        .await?;
    let usage: TokenUsage = sqlx::query_as("SELECT
        COALESCE(sum(input_tokens+output_tokens) FILTER(WHERE usage_source='reported'),0)::bigint AS reported,
        COALESCE(sum(input_tokens+output_tokens) FILTER(WHERE usage_source='estimated'),0)::bigint AS estimated,
        COALESCE(sum(input_tokens+output_tokens) FILTER(WHERE usage_source='mixed'),0)::bigint AS mixed,
        COALESCE(sum(reserved_input+reserved_output) FILTER(WHERE settled_at IS NULL),0)::bigint AS unconfirmed
        FROM model_usage_calls WHERE company_id=$1 AND root_run_id=$2")
        .bind(scope.company.as_uuid()).bind(root.root_run_id).fetch_one(db).await?;
    Ok(RootTokens {
        original: root
            .original
            .map(|n| u64::try_from(n).map_err(|_| invalid()))
            .transpose()?,
        allowance: u64::try_from(root.allowance).map_err(|_| invalid())?,
        refusal: root.token_refusal.map(decode_refusal).transpose()?,
        usage,
        model_exhausted: root.model_exhausted,
    })
}

fn decode_refusal(value: String) -> AppResult<crate::domain::workflow::TokenRefusal> {
    serde_json::from_value(Value::String(value)).map_err(|_| invalid())
}

/// Locks only the shared accounting row; callers already hold their local owner.
pub(super) async fn refusal_on(
    db: &mut PgConnection,
    scope: ActivationRequest,
) -> AppResult<Option<crate::domain::workflow::TokenRefusal>> {
    let refusal: Option<String> = sqlx::query_scalar(
        "SELECT usage.token_refusal FROM workflow_run_budgets AS link
        JOIN workflow_root_budget_usage AS usage USING(company_id,root_run_id)
        WHERE link.company_id=$1 AND link.run_id=$2 FOR UPDATE OF usage",
    )
    .bind(scope.company.as_uuid())
    .bind(scope.run.as_uuid())
    .fetch_one(db)
    .await?;
    refusal.map(decode_refusal).transpose()
}

pub(super) async fn require_progression(
    db: &mut PgConnection,
    scope: ActivationRequest,
) -> AppResult<()> {
    match progression_on(db, scope).await? {
        Progression::Open => Ok(()),
        gate => Err(AppError::Conflict(
            gate.failure_code().ok_or_else(invalid)?.into(),
        )),
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub(super) enum Progression {
    Open,
    AwaitingAllowance,
    Refused(crate::domain::workflow::TokenRefusal),
}

impl Progression {
    pub fn failure_code(self) -> Option<&'static str> {
        match self {
            Self::Open => None,
            Self::AwaitingAllowance => Some("workflow.tokens_awaiting_approval"),
            Self::Refused(reason) => Some(reason.failure_code()),
        }
    }
}

pub(super) async fn progression_on(
    db: &mut PgConnection,
    scope: ActivationRequest,
) -> AppResult<Progression> {
    lock_scope(db, scope).await?.progression()
}

pub(super) async fn pending_gate(
    db: &mut PgConnection,
    scope: ActivationRequest,
) -> AppResult<Progression> {
    let gate = progression_on(db, scope).await?;
    if gate != Progression::AwaitingAllowance {
        return Ok(gate);
    }
    // Approved, never-admitted native requests need a real fence to re-propose
    // their exact request if sibling usage has consumed the authorized total.
    let native:bool=sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM task_harness_runs AS checkpoint
        JOIN workflow_decisions AS decision ON decision.company_id=checkpoint.company_id AND decision.id=checkpoint.model_blocked_decision
        JOIN workflow_waits AS wait ON wait.company_id=decision.company_id AND wait.id=decision.wait_id
        WHERE checkpoint.company_id=$1 AND checkpoint.workflow_run_id=$2 AND checkpoint.workflow_execution_id=$3 AND checkpoint.task_id=$4
         AND checkpoint.state='active' AND checkpoint.model_request_id IS NULL
        AND decision.token_stage='reservation' AND decision.choice='approve' AND wait.state='completed')")
        .bind(scope.company.as_uuid()).bind(scope.run.as_uuid()).bind(scope.execution.as_uuid()).bind(scope.job.0).fetch_one(&mut *db).await?;
    if native {
        return Ok(Progression::Open);
    }
    let changed=sqlx::query("UPDATE background_tasks AS job SET run_at=LEAST(clock_timestamp()+interval '5 seconds',run.deadline),updated_at=clock_timestamp()
        FROM workflow_runs AS run WHERE job.company_id=$1 AND job.id=$2 AND job.workflow_execution_id=$3
        AND job.status='pending' AND job.worker_id IS NULL AND job.execution_generation IS NULL
        AND run.company_id=job.company_id AND run.id=$4 AND run.deadline>clock_timestamp()")
        .bind(scope.company.as_uuid()).bind(scope.job.0).bind(scope.execution.as_uuid()).bind(scope.run.as_uuid()).execute(db).await?.rows_affected();
    if changed != 1 {
        return Err(invalid());
    }
    Ok(gate)
}

/// Called before local owner/checkpoint/root locks, in the existing authority order.
#[derive(PartialEq, Eq)]
pub(super) struct TokenReviewerAccounts {
    root: Uuid,
    policy: Option<Value>,
    principals: Vec<Uuid>,
    mappings: Vec<super::approval_accounts::PrincipalAccount>,
}

impl TokenReviewerAccounts {
    pub(super) fn users(&self) -> impl Iterator<Item = Uuid> + '_ {
        self.mappings.iter().map(|mapping| mapping.user_id)
    }

    /// The model owner has already acquired this complete account union and company.
    pub(super) async fn available_on(
        &self,
        db: &mut PgConnection,
        company: CompanyId,
    ) -> AppResult<bool> {
        if self.mappings.len() != self.principals.len() {
            return Ok(false);
        }
        for mapping in &self.mappings {
            let actor = WorkflowActor::authenticated(mapping.user_id).map_err(|_| invalid())?;
            match authority::authorize_reviewer_on(db, company, actor).await {
                Ok(reviewer)
                    if reviewer.privileged && reviewer.principal == mapping.principal_id => {}
                Ok(_) | Err(AppError::NotFound(_)) => return Ok(false),
                Err(error) => return Err(error),
            }
        }
        Ok(true)
    }
}

pub(super) async fn reviewer_accounts_on(
    db: &mut PgConnection,
    scope: ActivationRequest,
) -> AppResult<TokenReviewerAccounts> {
    use crate::application::workflow::reviewer_assignment::ReviewerSelection;
    #[derive(sqlx::FromRow)]
    struct Policy {
        root_run_id: Uuid,
        token_policy: Option<Value>,
    }
    let saved: Policy = sqlx::query_as("SELECT link.root_run_id,budget.token_policy FROM workflow_run_budgets AS link \
        JOIN workflow_root_budgets AS budget USING(company_id,root_run_id) WHERE link.company_id=$1 AND link.run_id=$2")
        .bind(scope.company.as_uuid()).bind(scope.run.as_uuid()).fetch_one(&mut *db).await?;
    let principals: Vec<Uuid> = if let Some(policy) = &saved.token_policy {
        let selectors = crate::application::workflow::reviewers::parse_reviewers(
            policy.get("reviewers").ok_or_else(invalid)?,
        )?;
        let selected = ReviewerSelection::from_selectors(&selectors);
        if !selected.roles.is_empty() {
            return Err(invalid());
        }
        selected
            .principals
            .iter()
            .map(|principal| principal.as_uuid())
            .collect()
    } else {
        Vec::new()
    };
    let mappings = super::approval_accounts::mapped_on(db, scope.company, &principals).await?;
    Ok(TokenReviewerAccounts {
        root: saved.root_run_id,
        policy: saved.token_policy,
        principals,
        mappings,
    })
}
