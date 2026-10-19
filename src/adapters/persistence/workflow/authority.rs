use super::*;

/// A company member's current authority over decision commands (05-D2 §8, §9.1). The company
/// row is taken `FOR SHARE`, not `FOR NO KEY UPDATE` as controls do, so decision commands never
/// serialize with one another, yet still block owner and role changes until they commit.
pub(super) struct ReviewerAuthority {
    pub principal: Uuid,
    /// Owner or admin: required for a channel-less run.
    pub privileged: bool,
}
pub(super) async fn authorize_reviewer_on(
    db: &mut PgConnection,
    company: CompanyId,
    actor: WorkflowActor,
) -> AppResult<ReviewerAuthority> {
    let owner: Uuid = sqlx::query_scalar("SELECT user_id FROM companies WHERE id = $1 FOR SHARE")
        .bind(company.as_uuid())
        .fetch_optional(&mut *db)
        .await?
        .ok_or_else(missing)?;
    let (principal, role): (Uuid, String) = sqlx::query_as(
        "SELECT principal.id, member.role FROM company_members AS member \
         JOIN principals AS principal ON principal.company_id = member.company_id \
           AND principal.user_id = member.user_id AND principal.kind = 'person' \
         WHERE member.company_id = $1 AND member.user_id = $2 \
         ORDER BY principal.id LIMIT 1 \
         FOR SHARE OF member, principal",
    )
    .bind(company.as_uuid())
    .bind(actor.user_id())
    .fetch_optional(&mut *db)
    .await?
    .ok_or_else(missing)?;
    Ok(ReviewerAuthority {
        principal,
        privileged: owner == actor.user_id() || role == "admin" || role == "owner",
    })
}
/// Lock the actual owners, not a cached authorization result. Company-first is
/// also the MCP owner's configuration lock order. NO KEY UPDATE serializes
/// authority changes while allowing runtime foreign-key KEY SHARE checks;
/// FOR UPDATE here deadlocks completion holding a run against controls.
/// Member/principal row locks
/// prevent concurrent demotion/deletion until the workflow commit has finished.
pub(crate) async fn authorize_company(
    db: &mut PgConnection,
    company: CompanyId,
    actor: WorkflowActor,
) -> AppResult<()> {
    let owner: Uuid =
        sqlx::query_scalar("SELECT user_id FROM companies WHERE id = $1 FOR NO KEY UPDATE")
            .bind(company.as_uuid())
            .fetch_optional(&mut *db)
            .await?
            .ok_or_else(missing)?;
    let role: String = sqlx::query_scalar(
        "SELECT member.role FROM company_members AS member \
         JOIN principals AS principal ON principal.company_id = member.company_id \
           AND principal.user_id = member.user_id \
         WHERE member.company_id = $1 AND member.user_id = $2 \
         FOR SHARE OF member, principal",
    )
    .bind(company.as_uuid())
    .bind(actor.user_id())
    .fetch_optional(&mut *db)
    .await?
    .ok_or_else(missing)?;
    if owner != actor.user_id() && role != "admin" {
        return Err(missing());
    }
    Ok(())
}

/// Built-in messaging has no registration: a snapshot naming it must be exactly the trusted
/// built-in, and its current authority is the company's (04.11-D2 §1.6). `false` for every
/// other tool, which keeps its registration or approval check.
pub(super) fn builtin_message_tool(tool: &ToolSnapshot) -> AppResult<bool> {
    use crate::application::workflow::message::{
        is_builtin_message_tool, is_trusted_message_snapshot,
    };
    if !is_builtin_message_tool(tool.contract.name.as_str()) {
        return Ok(false);
    }
    if !is_trusted_message_snapshot(tool)? {
        return Err(missing());
    }
    Ok(true)
}

/// Tools must match a live HTTP registration or MCP approval for their slot kind.
/// Agents, skills and profiles have no authoritative durable owner yet: persisting
/// caller-supplied facts would turn frozen content into a grant (phase 06).
pub(super) async fn supported_publication(
    tx: &mut Transaction<'_, Postgres>,
    bundle: &PublishedBundle,
) -> AppResult<()> {
    let mut pending = vec![bundle];
    let mut seen = 0;
    while let Some(current) = pending.pop() {
        seen += 1;
        if seen > 256 || current.company_id() != bundle.company_id() {
            return Err(missing());
        }
        let snapshots = current.snapshots();
        if !snapshots.agents.is_empty()
            || !snapshots.skills.is_empty()
            || !snapshots.profiles.is_empty()
        {
            return Err(AppError::BadRequest(
                "Workflow dependency authority is not available".into(),
            ));
        }
        let definition = current.compiled().graph().definition();
        for tool in &snapshots.tools {
            if builtin_message_tool(tool)? {
                continue;
            }
            let kind = match &tool.contract.connection {
                Some(slot) => resources::slot_kind(definition, slot),
                None => Some("http"),
            };
            match kind {
                Some("http") => {
                    http::registered_on(tx, current.company_id(), tool).await?;
                }
                Some("mcp") => {
                    mcp::registered_on(tx, current.company_id(), tool).await?;
                }
                _ => return Err(missing()),
            }
        }
        pending.extend(current.children().values().map(AsRef::as_ref));
    }
    Ok(())
}
