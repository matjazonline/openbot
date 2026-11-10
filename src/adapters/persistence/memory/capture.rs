//! Capture policy settings (E8, U3): an owner or admin turns capture on or off per agent and
//! conversation kind, revision-checked, each change recording who, why and when. Lock order
//! (tenants.rs): users → company → membership → agent → `memory_tenants` FOR SHARE →
//! `memory_agent_capture` FOR UPDATE. A capture holds tenant → namespace → policy and never the
//! authority rows, so a switch change and a capture serialize on the policy row only.
use super::*;
use crate::application::memory::{
    CaptureLimits, MAX_CAPTURE_AGENTS, MemoryAgentCapture, MemoryCaptureChange,
    MemoryCaptureChangeRecord, MemoryCaptureKind, MemoryCapturePolicy,
};
use crate::application::workflow::{CompanyId, WorkflowActor};
use chrono::{DateTime, Utc};
use sqlx::PgConnection;

#[derive(sqlx::FromRow)]
struct PolicyRow {
    agent_id: Uuid,
    name: String,
    kind: Option<String>,
    enabled: Option<bool>,
    allowance_tokens: Option<i64>,
    max_output_tokens: Option<i32>,
    expiry_hours: Option<i32>,
    revision: Option<i64>,
    changed_by: Option<Uuid>,
    reason: Option<String>,
    changed_at: Option<DateTime<Utc>>,
}

/// The company's agents by name (at most `MAX_CAPTURE_AGENTS`) with both policies each; a
/// policy never set shows off with the default limits at revision 0.
pub(super) async fn capture_view_on(
    db: &mut PgConnection,
    company: Uuid,
) -> AppResult<(Vec<MemoryAgentCapture>, bool)> {
    let rows = sqlx::query_as::<_, PolicyRow>(
        "WITH listed AS (SELECT agent.id, agent.name FROM agents AS agent \
           WHERE agent.company_id = $1 ORDER BY agent.name, agent.id LIMIT $2) \
         SELECT listed.id AS agent_id, listed.name, policy.kind, policy.enabled, \
           policy.allowance_tokens, policy.max_output_tokens, policy.expiry_hours, \
           policy.revision, policy.changed_by, policy.reason, policy.changed_at \
         FROM listed LEFT JOIN memory_agent_capture AS policy \
           ON policy.company_id = $1 AND policy.agent_id = listed.id \
         ORDER BY listed.name, listed.id, policy.kind",
    )
    .bind(company)
    .bind(MAX_CAPTURE_AGENTS + 1)
    .fetch_all(&mut *db)
    .await?;
    let mut agents: Vec<MemoryAgentCapture> = Vec::new();
    for row in rows {
        if agents
            .last()
            .is_none_or(|agent| agent.agent != row.agent_id)
        {
            agents.push(MemoryAgentCapture {
                agent: row.agent_id,
                name: row.name.clone(),
                policies: MemoryCaptureKind::ALL
                    .into_iter()
                    .map(|kind| MemoryCapturePolicy {
                        kind,
                        enabled: false,
                        limits: CaptureLimits::DEFAULT,
                        revision: 0,
                        changed: None,
                    })
                    .collect(),
            });
        }
        let Some(kind) = row.kind.as_deref() else {
            continue;
        };
        let kind = MemoryCaptureKind::parse(kind).ok_or_else(invalid)?;
        let agent = agents.last_mut().ok_or_else(invalid)?;
        let policy = agent
            .policies
            .iter_mut()
            .find(|policy| policy.kind == kind)
            .ok_or_else(invalid)?;
        *policy = MemoryCapturePolicy {
            kind,
            enabled: row.enabled.ok_or_else(invalid)?,
            limits: CaptureLimits {
                allowance_tokens: row.allowance_tokens.ok_or_else(invalid)?,
                max_output_tokens: row.max_output_tokens.ok_or_else(invalid)?,
                expiry_hours: row.expiry_hours.ok_or_else(invalid)?,
            },
            revision: row.revision.ok_or_else(invalid)?,
            changed: Some(MemoryCaptureChangeRecord {
                by: row.changed_by.ok_or_else(invalid)?,
                reason: row.reason.ok_or_else(invalid)?,
                at: row.changed_at.ok_or_else(invalid)?,
            }),
        };
    }
    let more = agents.len() as i64 > MAX_CAPTURE_AGENTS;
    agents.truncate(MAX_CAPTURE_AGENTS as usize);
    Ok((agents, more))
}

fn invalid() -> AppError {
    AppError::Database("Invalid stored memory capture policy".into())
}

/// One policy change under the owner/admin authority locks (concealed `NotFound` otherwise).
pub(super) async fn set_capture_on(
    db: &mut PgConnection,
    company: CompanyId,
    actor: WorkflowActor,
    change: &MemoryCaptureChange,
) -> AppResult<()> {
    let id = company.as_uuid();
    super::tenants::lock_authority(db, company, actor).await?;
    sqlx::query("SELECT agent.id FROM agents AS agent WHERE agent.company_id = $1 AND agent.id = $2 FOR SHARE")
        .bind(id)
        .bind(change.agent)
        .fetch_optional(&mut *db)
        .await?
        .ok_or_else(|| AppError::NotFound("Agent not found".into()))?;
    let state: Option<String> = sqlx::query_scalar(
        "SELECT tenant.state FROM memory_tenants AS tenant WHERE tenant.company_id = $1 FOR SHARE",
    )
    .bind(id)
    .fetch_optional(&mut *db)
    .await?;
    match state.as_deref() {
        None => return Err(AppError::BadRequest("Memory is not enabled".into())),
        Some("enabled") => {}
        Some(_) if change.enabled => {
            return Err(AppError::BadRequest("Memory is disabled".into()));
        }
        Some(_) => {}
    }
    let current: Option<i64> = sqlx::query_scalar(
        "SELECT policy.revision FROM memory_agent_capture AS policy \
         WHERE policy.company_id = $1 AND policy.agent_id = $2 AND policy.kind = $3 FOR UPDATE",
    )
    .bind(id)
    .bind(change.agent)
    .bind(change.kind.as_str())
    .fetch_optional(&mut *db)
    .await?;
    if current.unwrap_or(0) != change.expected_revision {
        return Err(AppError::Conflict(
            "Memory settings changed; reload and retry".into(),
        ));
    }
    sqlx::query(
        "INSERT INTO memory_agent_capture AS policy (company_id, agent_id, kind, enabled, \
           allowance_tokens, max_output_tokens, expiry_hours, revision, changed_by, reason, \
           changed_at) \
         VALUES ($1, $2, $3, $4, $5, $6, $7, 1, $8, $9, clock_timestamp()) \
         ON CONFLICT (company_id, agent_id, kind) DO UPDATE SET enabled = EXCLUDED.enabled, \
           allowance_tokens = EXCLUDED.allowance_tokens, \
           max_output_tokens = EXCLUDED.max_output_tokens, expiry_hours = EXCLUDED.expiry_hours, \
           revision = policy.revision + 1, changed_by = EXCLUDED.changed_by, \
           reason = EXCLUDED.reason, changed_at = EXCLUDED.changed_at",
    )
    .bind(id)
    .bind(change.agent)
    .bind(change.kind.as_str())
    .bind(change.enabled)
    .bind(change.limits.allowance_tokens)
    .bind(change.limits.max_output_tokens)
    .bind(change.limits.expiry_hours)
    .bind(actor.user_id())
    .bind(&change.reason)
    .execute(&mut *db)
    .await?;
    tracing::info!(company_id = %id, agent_id = %change.agent, kind = change.kind.as_str(),
        enabled = change.enabled, changed_by = %actor.user_id(), "Memory capture policy changed");
    Ok(())
}
