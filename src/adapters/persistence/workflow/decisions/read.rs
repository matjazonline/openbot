//! Authorized decision projections. Commands and their lock/settlement protocol remain in the parent.
use super::*;

mod history;

#[derive(sqlx::FromRow)]
struct ViewRow {
    subject_kind: String,
    step_id: String,
    revision: i64,
    proposal_digest: String,
    proposal: Value,
    data_schema: Option<Value>,
    feedback_required: Option<Value>,
    deadline: DateTime<Utc>,
    state: String,
    submitted_by: Option<Uuid>,
    submitted_at: Option<DateTime<Utc>>,
    choice: Option<String>,
    feedback: Option<String>,
    data: Option<Value>,
}

#[derive(sqlx::FromRow)]
struct ContextRow {
    binding_id: Uuid,
    root_run_id: Uuid,
    version_id: Uuid,
    title: String,
    channel_id: Option<Uuid>,
    thread_id: Option<Uuid>,
    parent_run_id: Option<Uuid>,
    parent_execution_id: Option<Uuid>,
}

pub(super) async fn read_on(
    tx: &mut Transaction<'_, Postgres>,
    company: CompanyId,
    actor: WorkflowActor,
    decision: DecisionId,
) -> AppResult<DecisionView> {
    let Locked {
        scope, reviewer, ..
    } = lock_decision(tx, company, actor, decision).await?;
    let row: ViewRow = sqlx::query_as(READ_SQL)
        .bind(company.as_uuid())
        .bind(decision.0)
        .fetch_one(&mut **tx)
        .await?;
    let context: ContextRow = sqlx::query_as(
        "SELECT run.binding_id,budget.root_run_id,run.version_id,version.title,run.channel_id,run.thread_id,run.parent_run_id,run.parent_execution_id \
         FROM workflow_runs AS run JOIN workflow_versions AS version ON (version.company_id,version.id)=(run.company_id,run.version_id) \
         JOIN workflow_run_budgets AS budget ON (budget.company_id,budget.run_id)=(run.company_id,run.id) \
         WHERE run.company_id=$1 AND run.id=$2")
        .bind(company.as_uuid()).bind(scope.run.as_uuid()).fetch_one(&mut **tx).await?;
    let reviewers: Vec<Uuid> = sqlx::query_scalar("SELECT user_id FROM workflow_decision_reviewers WHERE company_id=$1 AND decision_id=$2 ORDER BY ordinal")
        .bind(company.as_uuid()).bind(decision.0).fetch_all(&mut **tx).await?;
    let comments = comments(tx, company, decision).await?;
    let subject = subject(&row.subject_kind)?;
    let step = StepId::parse(&row.step_id).map_err(|_| invalid())?;
    let choices = choices(tx, scope, context.binding_id, &step, subject).await?;
    let proposal = proposal(tx, company, decision, subject, &row.proposal).await?;
    let history = history::read(tx, scope, &context).await?;
    let trigger_message = sqlx::query_scalar(
        "SELECT intent.message_id FROM workflow_message_intent_bindings AS outcome \
         JOIN workflow_message_intents AS intent ON (intent.company_id,intent.id)=(outcome.company_id,outcome.intent_id) \
         WHERE outcome.company_id=$1 AND outcome.run_id=$2 LIMIT 1")
        .bind(company.as_uuid()).bind(context.root_run_id).fetch_optional(&mut **tx).await?;
    assemble(
        row,
        scope,
        decision,
        DecisionReadParts {
            subject,
            context: DecisionContext {
                company,
                workflow_title: context.title,
                version: crate::domain::workflow::VersionId::new(context.version_id),
                channel: context.channel_id,
                thread: context.thread_id,
                trigger_message,
                inspect_run: reviewer.privileged,
            },
            reviewers,
            actor: actor.user_id(),
            step,
            choices,
            proposal,
            comments,
            history,
        },
    )
}

struct DecisionReadParts {
    subject: DecisionSubject,
    context: DecisionContext,
    reviewers: Vec<Uuid>,
    actor: Uuid,
    step: StepId,
    choices: Vec<String>,
    proposal: Value,
    comments: Vec<DecisionCommentView>,
    history: history::History,
}

fn assemble(
    row: ViewRow,
    scope: ActivationRequest,
    decision: DecisionId,
    parts: DecisionReadParts,
) -> AppResult<DecisionView> {
    let feedback_required = row
        .feedback_required
        .map(serde_json::from_value)
        .transpose()
        .map_err(|_| invalid())?
        .unwrap_or_default();
    let submission = match (
        row.submitted_by,
        row.submitted_at,
        row.choice,
        row.feedback,
        row.data,
    ) {
        (Some(submitted_by), Some(submitted_at), Some(choice), Some(feedback), Some(data)) => {
            Some(DecisionSubmissionView {
                submitted_by,
                submitted_at,
                choice,
                feedback,
                data,
            })
        }
        (None, None, None, None, None) => None,
        _ => return Err(invalid()),
    };
    Ok(DecisionView {
        subject: parts.subject,
        context: parts.context,
        data_schema: if parts.subject == DecisionSubject::Business {
            row.data_schema
        } else {
            None
        },
        feedback_required,
        can_submit: row.deadline > Utc::now()
            && row.state == "waiting"
            && submission.is_none()
            && parts.reviewers.contains(&parts.actor),
        history: parts.history.items,
        history_truncated: parts.history.truncated,
        decision,
        run: scope.run,
        execution: scope.execution,
        step: parts.step,
        revision: row.revision,
        artifact_digest: row.proposal_digest,
        proposal: parts.proposal,
        deadline: row.deadline,
        state: row.state,
        reviewers: parts.reviewers,
        choices: parts.choices,
        submission,
        comments: parts.comments,
    })
}

fn subject(value: &str) -> AppResult<DecisionSubject> {
    match value {
        "business" => Ok(DecisionSubject::Business),
        "action_approval" => Ok(DecisionSubject::ProtectedAction),
        "token_allowance" => Ok(DecisionSubject::TokenAllowance),
        _ => Err(invalid()),
    }
}

async fn choices(
    tx: &mut Transaction<'_, Postgres>,
    scope: ActivationRequest,
    binding: Uuid,
    step: &StepId,
    subject: DecisionSubject,
) -> AppResult<Vec<String>> {
    if subject != DecisionSubject::Business {
        return Ok(vec!["approve".into(), "reject".into()]);
    }
    let binding = admission_binding::read_saved_binding(
        tx,
        scope.company,
        WorkflowBindingId::new(binding),
        scope.run.as_uuid(),
    )
    .await?;
    match binding
        .bundle()
        .compiled()
        .graph()
        .definition()
        .steps
        .get(step)
        .map(|step| &step.routes)
    {
        Some(Routes::Choices(routes)) => Ok(routes
            .iter()
            .map(|(choice, _)| choice.as_str().to_owned())
            .collect()),
        _ => Err(invalid()),
    }
}

async fn proposal(
    tx: &mut Transaction<'_, Postgres>,
    company: CompanyId,
    decision: DecisionId,
    subject: DecisionSubject,
    business: &Value,
) -> AppResult<Value> {
    match subject {
        DecisionSubject::Business => Ok(business.clone()),
        DecisionSubject::ProtectedAction => approvals::view_proposal(tx, company, decision).await,
        DecisionSubject::TokenAllowance => Ok(sqlx::query_scalar(
            "SELECT token_proposal FROM workflow_decisions WHERE company_id=$1 AND id=$2",
        )
        .bind(company.as_uuid())
        .bind(decision.0)
        .fetch_one(&mut **tx)
        .await?),
    }
}

async fn comments(
    tx: &mut Transaction<'_, Postgres>,
    company: CompanyId,
    decision: DecisionId,
) -> AppResult<Vec<DecisionCommentView>> {
    #[derive(sqlx::FromRow)]
    struct Row {
        sequence: i64,
        author_principal: Uuid,
        body: String,
        created_at: DateTime<Utc>,
    }
    let rows:Vec<Row>=sqlx::query_as("SELECT sequence,author_principal_id AS author_principal,body,created_at FROM workflow_decision_comments WHERE company_id=$1 AND decision_id=$2 ORDER BY sequence LIMIT $3")
        .bind(company.as_uuid()).bind(decision.0).bind(MAX_DECISION_COMMENTS).fetch_all(&mut **tx).await?;
    Ok(rows
        .into_iter()
        .map(|row| DecisionCommentView {
            sequence: row.sequence,
            author_principal: row.author_principal,
            body: row.body,
            created_at: row.created_at,
        })
        .collect())
}

const READ_SQL: &str = r#"
SELECT decision.subject_kind,execution.step_id,decision.revision,decision.proposal_digest,
       COALESCE(execution.frozen_inputs->'proposal','null'::jsonb) AS proposal,
       execution.frozen_inputs->'data_schema' AS data_schema,
       execution.frozen_inputs->'feedback_required' AS feedback_required,
       wait.deadline,wait.state,decision.submitted_by,decision.submitted_at,decision.choice,decision.feedback,decision.data
FROM workflow_decisions AS decision
JOIN workflow_executions AS execution ON (execution.company_id,execution.id)=(decision.company_id,decision.execution_id)
JOIN workflow_waits AS wait ON (wait.company_id,wait.id)=(decision.company_id,decision.wait_id)
WHERE decision.company_id=$1 AND decision.id=$2
"#;
