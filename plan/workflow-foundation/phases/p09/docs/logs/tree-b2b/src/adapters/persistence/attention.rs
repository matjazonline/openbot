use async_trait::async_trait;
use chrono::{DateTime, Utc};
use uuid::Uuid;

use crate::{
    adapters::persistence::PostgresPersistence,
    app_error::{AppError, AppResult},
    application::attention::AttentionPersistence,
    entities::{
        attention::{
            AttentionCursor, AttentionItem, AttentionPage, AttentionQuery, AttentionResponsibility,
            AttentionSourceKind, AttentionView, OperationalSummary,
        },
        correlation::CorrelationId,
        transport::PrincipalId,
    },
};

#[derive(sqlx::FromRow)]
struct AttentionRow {
    source_kind: String,
    source_id: Uuid,
    company_id: Uuid,
    channel_id: Option<Uuid>,
    thread_id: Option<Uuid>,
    run_id: Option<Uuid>,
    correlation_id: Option<Uuid>,
    state: String,
    responsible_principal_id: Option<Uuid>,
    responsibility_kind: String,
    responsibility_label: String,
    title: String,
    next_action: String,
    business_priority: String,
    due_at: Option<DateTime<Utc>>,
    expires_at: Option<DateTime<Utc>>,
    version: i64,
    created_at: DateTime<Utc>,
    updated_at: DateTime<Utc>,
    as_of: DateTime<Utc>,
    bounded_count: i64,
}

impl TryFrom<AttentionRow> for AttentionItem {
    type Error = AppError;

    fn try_from(row: AttentionRow) -> AppResult<Self> {
        Ok(Self {
            source_kind: row.source_kind.parse().map_err(AppError::Internal)?,
            source_id: row.source_id,
            company_id: row.company_id,
            channel_id: row.channel_id,
            thread_id: row.thread_id,
            run_id: row.run_id,
            correlation_id: row.correlation_id.map(CorrelationId::from),
            state: row.state,
            responsibility: match (
                row.responsibility_kind.as_str(),
                row.responsible_principal_id,
            ) {
                ("principal", Some(id)) => AttentionResponsibility::Principal(PrincipalId::new(id)),
                ("channel_team", None) => AttentionResponsibility::ChannelTeam,
                ("external", None) => AttentionResponsibility::External,
                (kind, principal_id) => {
                    return Err(AppError::Internal(format!(
                        "Invalid attention responsibility {kind} with principal {principal_id:?}"
                    )));
                }
            },
            responsibility_label: row.responsibility_label,
            title: row.title,
            next_action: row.next_action,
            priority: row.business_priority.parse().map_err(AppError::Internal)?,
            due_at: row.due_at,
            expires_at: row.expires_at,
            version: u64::try_from(row.version)
                .map_err(|_| AppError::Internal("Invalid attention source version".into()))?,
            created_at: row.created_at,
            updated_at: row.updated_at,
            href: None,
        })
    }
}

/// The work list (p08 D4.2). `$14` is whether the viewer is an owner or admin: company-associated
/// sources (no channel) are visible only to them, as on the decision page.
const ATTENTION_SQL: &str = r#"
WITH params AS (
    SELECT COALESCE($5::timestamptz, CURRENT_TIMESTAMP) AS as_of
), raw AS (
    -- Every branch applies visibility (a readable channel, or company-associated for a manager)
    -- and keeps to the views its own columns imply, so `my_work` and `unassigned` never build a
    -- teammate's rows only for `ranked` to discard them. Each view filter reads
    -- `$4 <> '<view>' OR <predicate>`. Decisions are the one source that is narrower than
    -- `ranked` on purpose: they are never unassigned work.

    -- My work: the open decisions the viewer reviews, driven from `workflow_decisions_open` and
    -- probed on the reviewer key, so the work stays bounded by open decisions however many the
    -- viewer has reviewed before.
    SELECT 'decision'::text AS source_kind, decision.id AS source_id, decision.company_id,
           run.channel_id, run.thread_id, run.id AS run_id, run.correlation_id,
           'open'::text AS state,
           viewer.id AS responsible_principal_id, 'principal'::text AS responsibility_kind,
           viewer.display_label AS responsibility_label,
           version.title,
           CASE decision.subject_kind
             WHEN 'action_approval' THEN 'Approve or reject the action'
             WHEN 'token_allowance' THEN 'Approve or reject the token allowance'
             ELSE 'Review and decide'
           END AS next_action,
           'normal'::text AS business_priority, wait.deadline AS due_at,
           NULL::timestamptz AS expires_at, decision.revision AS version,
           decision.created_at, decision.created_at AS updated_at
    FROM principals AS viewer
    JOIN workflow_decision_reviewers AS reviewer
      ON reviewer.company_id = viewer.company_id AND reviewer.user_id = viewer.user_id
     AND reviewer.principal_id = viewer.id
    JOIN workflow_decisions AS decision
      ON decision.company_id = reviewer.company_id AND decision.id = reviewer.decision_id
    JOIN workflow_waits AS wait
      ON wait.company_id = decision.company_id AND wait.id = decision.wait_id
    JOIN workflow_runs AS run
      ON run.company_id = decision.company_id AND run.id = decision.run_id
    JOIN workflow_versions AS version
      ON version.company_id = run.company_id AND version.id = run.version_id
    WHERE $4 = 'my_work' AND viewer.company_id = $1 AND viewer.id = $3
      AND viewer.kind = 'person'
      AND decision.submitted_at IS NULL AND wait.state = 'waiting'
      AND (run.channel_id = ANY($2) OR (run.channel_id IS NULL AND $14))

    UNION ALL

    -- Team work: every open decision, one row each, whoever reviews it.
    SELECT 'decision', decision.id, decision.company_id, run.channel_id, run.thread_id, run.id,
           run.correlation_id, 'open', NULL::uuid, 'channel_team',
           (SELECT CASE COUNT(*) WHEN 1 THEN '1 reviewer' ELSE COUNT(*) || ' reviewers' END
            FROM workflow_decision_reviewers AS reviewer
            WHERE reviewer.company_id = decision.company_id
              AND reviewer.decision_id = decision.id),
           version.title,
           CASE decision.subject_kind
             WHEN 'action_approval' THEN 'Approve or reject the action'
             WHEN 'token_allowance' THEN 'Approve or reject the token allowance'
             ELSE 'Review and decide'
           END,
           'normal', wait.deadline, NULL::timestamptz, decision.revision,
           decision.created_at, decision.created_at
    FROM workflow_decisions AS decision
    JOIN workflow_waits AS wait
      ON wait.company_id = decision.company_id AND wait.id = decision.wait_id
    JOIN workflow_runs AS run
      ON run.company_id = decision.company_id AND run.id = decision.run_id
    JOIN workflow_versions AS version
      ON version.company_id = run.company_id AND version.id = run.version_id
    WHERE $4 = 'team_work' AND decision.company_id = $1
      AND decision.submitted_at IS NULL AND wait.state = 'waiting'
      AND (run.channel_id = ANY($2) OR (run.channel_id IS NULL AND $14))

    UNION ALL

    SELECT 'run_reconciliation', run.id, run.company_id, run.channel_id, run.thread_id, run.id,
           run.correlation_id, run.state, NULL::uuid, 'channel_team',
           CASE WHEN run.channel_id IS NULL THEN 'Owners and admins' ELSE 'Channel team' END,
           version.title, 'Check whether the uncertain action took effect', 'normal',
           run.deadline, NULL::timestamptz, run.revision, run.created_at, run.created_at
    FROM workflow_runs AS run
    JOIN workflow_versions AS version
      ON version.company_id = run.company_id AND version.id = run.version_id
    WHERE $4 <> 'my_work' AND run.company_id = $1
      AND run.state = 'waiting' AND run.waiting_reason = 'reconciliation'
      AND (run.channel_id = ANY($2) OR (run.channel_id IS NULL AND $14))

    UNION ALL

    -- The run actor's work while they are a member; nobody's otherwise.
    SELECT 'run_failure', run.id, run.company_id, run.channel_id, run.thread_id, run.id,
           run.correlation_id, run.state, actor.id,
           CASE WHEN actor.id IS NULL THEN 'channel_team' ELSE 'principal' END,
           COALESCE(actor.display_label,
                    CASE WHEN run.channel_id IS NULL THEN 'Owners and admins'
                         ELSE 'Channel team' END),
           version.title, 'Review the failed run (failed in the last 7 days)', 'normal',
           NULL::timestamptz, NULL::timestamptz, run.revision, run.created_at, run.created_at
    FROM workflow_runs AS run
    CROSS JOIN params
    JOIN workflow_versions AS version
      ON version.company_id = run.company_id AND version.id = run.version_id
    LEFT JOIN principals AS actor
      ON actor.company_id = run.company_id AND actor.user_id = run.actor_id
     AND actor.kind = 'person'
     AND EXISTS (SELECT 1 FROM company_members AS member
                 WHERE member.company_id = run.company_id AND member.user_id = run.actor_id)
    WHERE run.company_id = $1 AND run.state = 'failed'
      AND run.created_at >= params.as_of - interval '7 days'
      AND NOT EXISTS (SELECT 1 FROM workflow_runs AS rerun
                      WHERE rerun.company_id = run.company_id AND rerun.rerun_of_run_id = run.id)
      AND (run.channel_id = ANY($2) OR (run.channel_id IS NULL AND $14))
      AND ($4 <> 'my_work' OR actor.id = $3)
      AND ($4 <> 'unassigned' OR actor.id IS NULL)

    UNION ALL

    -- One row per binding. Refusals create no run and no event, so this refreshes on the page's
    -- periodic tick.
    SELECT 'binding_refusal', binding.id, binding.company_id, binding.channel_id, NULL::uuid,
           NULL::uuid, NULL::uuid, 'refused', NULL::uuid, 'channel_team',
           CASE WHEN binding.channel_id IS NULL THEN 'Owners and admins' ELSE 'Channel team' END,
           COALESCE(version.title, 'Workflow binding'),
           refusal.refused || CASE WHEN refusal.refused = 1 THEN ' refusal' ELSE ' refusals' END
               || ' in the last 7 days; latest: ' || refusal.latest_reason,
           'normal', NULL::timestamptz, NULL::timestamptz, refusal.refused,
           refusal.latest_at, refusal.latest_at
    FROM (
        SELECT recent.binding_id, COUNT(*) AS refused, MAX(recent.created_at) AS latest_at,
               (ARRAY_AGG(recent.reason ORDER BY recent.created_at DESC))[1] AS latest_reason
        FROM (
            SELECT intent_binding.binding_id, intent_binding.created_at, intent_binding.reason
            FROM workflow_message_intent_bindings AS intent_binding
            CROSS JOIN params
            WHERE $4 = 'team_work' AND intent_binding.company_id = $1
              AND intent_binding.outcome <> 'admitted'
              AND intent_binding.created_at >= params.as_of - interval '7 days'
            UNION ALL
            SELECT schedule_run.binding_id, schedule_run.created_at,
                   LEFT(COALESCE(schedule_run.refusal, schedule_run.last_error,
                                 'schedule materialization failed'), 200)
            FROM schedule_runs AS schedule_run
            CROSS JOIN params
            WHERE $4 = 'team_work' AND schedule_run.company_id = $1
              AND schedule_run.materialization_status = 'failed'
              AND schedule_run.created_at >= params.as_of - interval '7 days'
        ) AS recent
        GROUP BY recent.binding_id
    ) AS refusal
    JOIN workflow_bindings AS binding
      ON binding.company_id = $1 AND binding.id = refusal.binding_id
    LEFT JOIN workflow_binding_revisions AS revision
      ON revision.company_id = binding.company_id AND revision.binding_id = binding.id
     AND revision.revision = binding.configuration_revision
    LEFT JOIN workflow_versions AS version
      ON version.company_id = revision.company_id AND version.id = revision.version_id
    WHERE $4 = 'team_work'
      AND (binding.channel_id = ANY($2) OR (binding.channel_id IS NULL AND $14))

    UNION ALL

    -- A workflow delivery is its run actor's work while they are a member; any other company
    -- delivery (a person's mailbox send) is the channel team's.
    SELECT 'delivery_failure', delivery.id, delivery.company_id, delivery.channel_id,
           destination.thread_id, run.id, delivery.correlation_id, delivery.status, actor.id,
           CASE WHEN actor.id IS NULL THEN 'channel_team' ELSE 'principal' END,
           COALESCE(actor.display_label, 'Channel team'),
           message.subject,
           CASE delivery.status WHEN 'outcome_unknown' THEN 'Resolve the unknown delivery outcome'
                ELSE 'Repair or dismiss the permanent delivery failure' END,
           'normal', NULL::timestamptz, NULL::timestamptz,
           GREATEST(delivery.attempt_count, 1)::bigint, delivery.created_at, delivery.updated_at
    FROM message_deliveries AS delivery
    JOIN messages AS message
      ON message.company_id = delivery.company_id AND message.id = delivery.message_id
    LEFT JOIN workflow_message_destinations AS destination
      ON destination.company_id = delivery.company_id AND destination.delivery_id = delivery.id
    LEFT JOIN workflow_runs AS run
      ON run.company_id = destination.company_id AND run.id = destination.run_id
    LEFT JOIN principals AS actor
      ON actor.company_id = run.company_id AND actor.user_id = run.actor_id
     AND actor.kind = 'person'
     AND EXISTS (SELECT 1 FROM company_members AS member
                 WHERE member.company_id = run.company_id AND member.user_id = run.actor_id)
    WHERE delivery.company_id = $1 AND delivery.channel_id = ANY($2)
      AND delivery.status IN ('outcome_unknown', 'dead_letter')
      AND delivery.last_error_class IS DISTINCT FROM 'superseded'
      AND ($4 <> 'my_work' OR actor.id = $3)
      AND ($4 <> 'unassigned' OR actor.id IS NULL)
), ranked AS (
    -- The authoritative responsibility filter. A branch filter may be removed without changing
    -- a row, but must never be narrower than this one unless that is the product rule (the
    -- decisions above), because `ranked` cannot restore a row a branch never produced.
    SELECT raw.*, params.as_of,
           CASE WHEN raw.due_at < params.as_of THEN 0
                WHEN raw.due_at <= params.as_of + interval '24 hours' THEN 1 ELSE 2 END AS due_rank,
           CASE raw.business_priority WHEN 'urgent' THEN 0 WHEN 'high' THEN 1 ELSE 2 END
               AS priority_rank
    FROM raw CROSS JOIN params
    WHERE ($4 = 'team_work'
           OR ($4 = 'my_work' AND raw.responsibility_kind = 'principal'
                                  AND raw.responsible_principal_id = $3)
           OR ($4 = 'unassigned' AND raw.responsibility_kind = 'channel_team'))
), after_cursor AS (
    -- Every page sorts the whole filtered set, and that is inherent: `due_rank` depends on
    -- `as_of`, so no index can supply this order. The branch filters shrink the input for
    -- `my_work` and `unassigned`; `team_work` sorts everything, bounded by the working set.
    SELECT * FROM ranked
    WHERE NOT $6 OR (due_rank, priority_rank, created_at, source_kind, source_id)
          > ($7, $8, $9, $10, $11)
    ORDER BY due_rank, priority_rank, created_at, source_kind, source_id
    LIMIT $12
), counted AS (
    SELECT after_cursor.*, COUNT(*) OVER () AS bounded_count FROM after_cursor
)
SELECT * FROM counted
ORDER BY due_rank, priority_rank, created_at, source_kind, source_id
LIMIT $13
"#;

/// Bind a feed statement to the fourteen parameters `query` implies, examining at most
/// `working_set` eligible rows after the cursor.
///
/// Takes the statement rather than naming `ATTENTION_SQL` so a test can hold a reference
/// statement to exactly the parameters production binds.
fn bind_attention<'q>(
    sql: &'q str,
    query: AttentionQuery<'q>,
    working_set: usize,
) -> sqlx::query::QueryAs<'q, sqlx::Postgres, AttentionRow, sqlx::postgres::PgArguments> {
    let cursor = query.cursor;
    let view = match query.view {
        AttentionView::MyWork => "my_work",
        AttentionView::Unassigned => "unassigned",
        AttentionView::TeamWork => "team_work",
    };
    let sentinel_time = DateTime::<Utc>::UNIX_EPOCH;
    let sentinel_id = Uuid::nil();
    sqlx::query_as::<_, AttentionRow>(sql)
        .bind(query.company_id)
        .bind(query.visible_channel_ids)
        .bind(query.principal_id.as_uuid())
        .bind(view)
        .bind(cursor.map(|cursor| cursor.as_of))
        .bind(cursor.is_some())
        .bind(cursor.map_or(0_i32, |cursor| i32::from(cursor.due_rank)))
        .bind(cursor.map_or(0_i32, |cursor| i32::from(cursor.priority_rank)))
        .bind(cursor.map_or(sentinel_time, |cursor| cursor.created_at))
        .bind(cursor.map_or("", |cursor| cursor.source_kind.as_str()))
        .bind(cursor.map_or(sentinel_id, |cursor| cursor.source_id))
        .bind((working_set + 1) as i64)
        .bind((query.clamped_limit() + 1) as i64)
        .bind(query.manager)
}

/// Stable destinations; their routes independently authorize company and channel access.
fn item_href(item: &AttentionItem) -> Option<String> {
    match item.source_kind {
        AttentionSourceKind::Decision => Some(format!("/ui/workflow-decisions/{}", item.source_id)),
        AttentionSourceKind::DeliveryFailure => {
            Some(format!("/ui/deliveries?company_id={}", item.company_id))
        }
        AttentionSourceKind::RunReconciliation | AttentionSourceKind::RunFailure => {
            item.run_id.map(|run| {
                format!(
                    "/ui/workflow-runs?company_id={}&run_id={run}",
                    item.company_id
                )
            })
        }
        AttentionSourceKind::BindingRefusal => Some(format!(
            "/ui/workflow-bindings/{}?company_id={}",
            item.source_id, item.company_id
        )),
    }
}

impl PostgresPersistence {
    /// [`AttentionPersistence::list_attention`] with the working set bounded at `working_set`, so
    /// a test reaches truncation with a few rows rather than a thousand.
    async fn attention_page(
        &self,
        query: AttentionQuery<'_>,
        working_set: usize,
    ) -> AppResult<AttentionPage> {
        let limit = query.clamped_limit();
        let as_of = query.cursor.map(|cursor| cursor.as_of);
        let rows = bind_attention(ATTENTION_SQL, query, working_set)
            .fetch_all(&self.pool)
            .await
            .map_err(AppError::from)?;

        let bounded_count = rows.first().map_or(0, |row| row.bounded_count);
        let result_as_of = rows
            .first()
            .map_or_else(|| as_of.unwrap_or_else(Utc::now), |row| row.as_of);
        let has_next = rows.len() > limit;
        let mut items = rows
            .into_iter()
            .take(limit)
            .map(TryInto::try_into)
            .collect::<AppResult<Vec<AttentionItem>>>()?;
        for item in &mut items {
            item.href = match item.source_kind {
                AttentionSourceKind::RunFailure
                | AttentionSourceKind::RunReconciliation
                | AttentionSourceKind::BindingRefusal
                    if !query.manager =>
                {
                    None
                }
                _ => item_href(item),
            };
        }
        let next_cursor = has_next
            .then(|| {
                items
                    .last()
                    .map(|item| AttentionCursor::for_item(item, result_as_of).to_string())
            })
            .flatten();
        Ok(AttentionPage {
            items,
            next_cursor,
            as_of: result_as_of,
            working_set_size: usize::try_from(bounded_count).map_err(|_| {
                AppError::Internal("Attention working-set count cannot be negative".into())
            })?,
            truncated: bounded_count > working_set as i64,
        })
    }
}

#[async_trait]
impl AttentionPersistence for PostgresPersistence {
    async fn list_attention(&self, query: AttentionQuery<'_>) -> AppResult<AttentionPage> {
        self.attention_page(query, AttentionQuery::MAX_WORKING_SET)
            .await
    }

    async fn operational_summary(
        &self,
        company_id: Uuid,
        visible_channel_ids: &[Uuid],
    ) -> AppResult<OperationalSummary> {
        #[derive(sqlx::FromRow)]
        struct SummaryRow {
            as_of: DateTime<Utc>,
            open_decision_count: i64,
            oldest_open_decision_age_seconds: Option<f64>,
            runs_awaiting_reconciliation: i64,
            failed_run_count: i64,
            binding_refusal_count: i64,
            permanent_delivery_failure_count: i64,
        }
        // The team-work branches of `ATTENTION_SQL` for a manager, counted. Keep the predicates
        // in step with them; `the_summary_counts_what_team_work_lists` holds the two together.
        let row = sqlx::query_as::<_, SummaryRow>(
            r#"WITH clock AS (SELECT CURRENT_TIMESTAMP AS as_of),
               open_decisions AS (
                 SELECT decision.created_at
                 FROM workflow_decisions AS decision
                 JOIN workflow_waits AS wait
                   ON wait.company_id = decision.company_id AND wait.id = decision.wait_id
                 JOIN workflow_runs AS run
                   ON run.company_id = decision.company_id AND run.id = decision.run_id
                 WHERE decision.company_id = $1 AND decision.submitted_at IS NULL
                   AND wait.state = 'waiting'
                   AND (run.channel_id = ANY($2) OR run.channel_id IS NULL)
               )
               SELECT clock.as_of,
                 (SELECT COUNT(*) FROM open_decisions) AS open_decision_count,
                 (SELECT EXTRACT(EPOCH FROM (clock.as_of - MIN(created_at)))::float8
                    FROM open_decisions) AS oldest_open_decision_age_seconds,
                 (SELECT COUNT(*) FROM workflow_runs AS run
                    WHERE run.company_id = $1 AND run.state = 'waiting'
                      AND run.waiting_reason = 'reconciliation'
                      AND (run.channel_id = ANY($2) OR run.channel_id IS NULL))
                    AS runs_awaiting_reconciliation,
                 (SELECT COUNT(*) FROM workflow_runs AS run
                    WHERE run.company_id = $1 AND run.state = 'failed'
                      AND run.created_at >= clock.as_of - interval '7 days'
                      AND NOT EXISTS (SELECT 1 FROM workflow_runs AS rerun
                                      WHERE rerun.company_id = run.company_id
                                        AND rerun.rerun_of_run_id = run.id)
                      AND (run.channel_id = ANY($2) OR run.channel_id IS NULL))
                    AS failed_run_count,
                 (SELECT COUNT(*) FROM (
                      SELECT intent_binding.binding_id
                      FROM workflow_message_intent_bindings AS intent_binding
                      WHERE intent_binding.company_id = $1
                        AND intent_binding.outcome <> 'admitted'
                        AND intent_binding.created_at >= clock.as_of - interval '7 days'
                      UNION ALL
                      SELECT schedule_run.binding_id FROM schedule_runs AS schedule_run
                      WHERE schedule_run.company_id = $1
                        AND schedule_run.materialization_status = 'failed'
                        AND schedule_run.created_at >= clock.as_of - interval '7 days'
                    ) AS recent
                    JOIN workflow_bindings AS binding
                      ON binding.company_id = $1 AND binding.id = recent.binding_id
                    WHERE binding.channel_id = ANY($2) OR binding.channel_id IS NULL)
                    AS binding_refusal_count,
                 (SELECT COUNT(*) FROM message_deliveries AS delivery
                    WHERE delivery.company_id = $1 AND delivery.channel_id = ANY($2)
                      AND delivery.status = 'dead_letter'
                      AND delivery.last_error_class IS DISTINCT FROM 'superseded')
                    AS permanent_delivery_failure_count
               FROM clock"#,
        )
        .bind(company_id)
        .bind(visible_channel_ids)
        .fetch_one(&self.pool)
        .await
        .map_err(AppError::from)?;
        Ok(OperationalSummary {
            as_of: row.as_of,
            open_decision_count: row.open_decision_count,
            oldest_open_decision_age_seconds: row.oldest_open_decision_age_seconds,
            runs_awaiting_reconciliation: row.runs_awaiting_reconciliation,
            failed_run_count: row.failed_run_count,
            binding_refusal_count: row.binding_refusal_count,
            permanent_delivery_failure_count: row.permanent_delivery_failure_count,
        })
    }
}

#[cfg(test)]
#[path = "attention_tests.rs"]
mod tests;
