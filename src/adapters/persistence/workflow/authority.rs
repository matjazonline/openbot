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
) -> AppResult<crate::entities::access_level::LevelActor> {
    use crate::adapters::persistence::participant::{membership_from_db, reader_level};
    use crate::entities::access_level::LevelActor;
    let owner: Uuid =
        sqlx::query_scalar("SELECT user_id FROM companies WHERE id = $1 FOR NO KEY UPDATE")
            .bind(company.as_uuid())
            .fetch_optional(&mut *db)
            .await?
            .ok_or_else(missing)?;
    #[derive(sqlx::FromRow)]
    struct MemberAuthority {
        role: String,
        access_level: i16,
    }
    let facts=sqlx::query_as::<_,MemberAuthority>(
        "SELECT member.role, member.access_level::smallint AS access_level FROM company_members AS member \
         JOIN principals AS principal ON principal.company_id=member.company_id AND principal.user_id=member.user_id \
         WHERE member.company_id=$1 AND member.user_id=$2 AND principal.kind='person' \
         FOR SHARE OF member,principal")
        .bind(company.as_uuid()).bind(actor.user_id()).fetch_optional(db).await?.ok_or_else(missing)?;
    let membership = membership_from_db(Some(&facts.role))?;
    if membership.is_owner() != (owner == actor.user_id()) {
        return Err(invalid());
    }
    let access_level = reader_level(actor.user_id(), membership, Some(facts.access_level))?;
    if !membership.manages_company_operations() {
        return Err(missing());
    }
    Ok(LevelActor {
        membership,
        access_level,
    })
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

/// Built-in provisioning has no registration either: a name-only snapshot under a provisioning
/// name must be exactly the trusted built-in (p08 D2.1).
pub(super) fn builtin_provisioning_tool(tool: &ToolSnapshot) -> AppResult<()> {
    if !crate::application::workflow::provisioning::is_trusted_provisioning_snapshot(tool)? {
        return Err(missing());
    }
    Ok(())
}

/// Tools must match a live HTTP registration or MCP approval for their slot kind. Agent and
/// skill snapshots are frozen content verified once, at publish, against their durable
/// owners (`agent_publication::verify_on`, 06-D5); later agent edits never block stored
/// versions, their bindings, admission or parents.
///
/// Workflow tools (07-D2 §D2.2): `check` applies to the root's own snapshots only; stored child
/// bundles were verified at their own publication and are held to the run rule.
pub(super) async fn supported_publication(
    tx: &mut Transaction<'_, Postgres>,
    bundle: &PublishedBundle,
    check: workflow_tools::ToolCheck,
) -> AppResult<()> {
    if matches!(check, workflow_tools::ToolCheck::Publish) {
        jev::publication_on(tx, bundle).await?;
    }
    let mut pending = vec![(bundle, check)];
    let mut seen = 0;
    while let Some((current, check)) = pending.pop() {
        seen += 1;
        if seen > 256 || current.company_id() != bundle.company_id() {
            return Err(missing());
        }
        let snapshots = current.snapshots();
        let definition = current.compiled().graph().definition();
        for tool in &snapshots.tools {
            match tool.kind() {
                ToolKind::Message => {
                    builtin_message_tool(tool)?;
                }
                ToolKind::Provisioning => builtin_provisioning_tool(tool)?,
                ToolKind::Knowledge => builtin_knowledge_tool(tool)?,
                ToolKind::Memory => builtin_memory_tool(tx, tool, check).await?,
                ToolKind::Http => {
                    http::registered_on(tx, current.company_id(), tool).await?;
                }
                ToolKind::Slot(slot) => match resources::slot_kind(definition, slot) {
                    Some("http") => {
                        http::registered_on(tx, current.company_id(), tool).await?;
                    }
                    Some("mcp") => {
                        mcp::registered_on(tx, current.company_id(), tool).await?;
                    }
                    _ => return Err(missing()),
                },
                ToolKind::Workflow(_) => {
                    workflow_tools::registered_on(tx, current.company_id(), tool, check).await?;
                }
            }
        }
        pending.extend(
            current
                .children()
                .values()
                .map(|child| (child.as_ref(), workflow_tools::ToolCheck::Run)),
        );
    }
    Ok(())
}

/// A trusted memory built-in, published only while the company has memory enabled (phase 05
/// U1/E5). A published one keeps running after a disable: each call then answers
/// `memory.not_enabled` (06-D5 revocation at runtime) instead of failing the whole run.
pub(super) async fn builtin_memory_tool(
    db: &mut PgConnection,
    tool: &ToolSnapshot,
    check: workflow_tools::ToolCheck,
) -> AppResult<()> {
    if crate::application::workflow::memory_tools::trusted_memory_tool(tool)?.is_none() {
        return Err(missing());
    }
    if matches!(check, workflow_tools::ToolCheck::Publish)
        && !crate::adapters::persistence::memory::memory_enabled_on(db, tool.company_id).await?
    {
        return Err(AppError::NotFound(
            "Memory is not enabled for this company".into(),
        ));
    }
    Ok(())
}

pub(super) fn builtin_knowledge_tool(tool: &ToolSnapshot) -> AppResult<()> {
    if crate::application::workflow::knowledge::is_trusted_knowledge_snapshot(tool)? {
        Ok(())
    } else {
        Err(missing())
    }
}
