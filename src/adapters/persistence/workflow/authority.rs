use super::*;

/// Lock the actual owners, not a cached authorization result. Company-first is
/// also the MCP owner's configuration lock order. NO KEY UPDATE serializes
/// authority changes while allowing runtime foreign-key KEY SHARE checks;
/// FOR UPDATE here deadlocks completion holding a run against controls.
/// Member/principal row locks
/// prevent concurrent demotion/deletion until the workflow commit has finished.
pub(super) async fn authorize_company(
    tx: &mut Transaction<'_, Postgres>,
    company: CompanyId,
    actor: WorkflowActor,
) -> AppResult<()> {
    let owner: Uuid =
        sqlx::query_scalar("SELECT user_id FROM companies WHERE id = $1 FOR NO KEY UPDATE")
            .bind(company.as_uuid())
            .fetch_optional(&mut **tx)
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
    .fetch_optional(&mut **tx)
    .await?
    .ok_or_else(missing)?;
    if owner != actor.user_id() && role != "admin" {
        return Err(missing());
    }
    Ok(())
}

/// These facts have no matching authoritative durable owners yet. Persisting
/// caller-supplied policy/profile/skill facts would turn frozen content into a
/// grant. Their phase04/06 adapters must replace this fail-closed boundary.
pub(super) fn supported_publication(bundle: &PublishedBundle) -> AppResult<()> {
    let snapshots = bundle.snapshots();
    if !snapshots.agents.is_empty()
        || !snapshots.skills.is_empty()
        || !snapshots.profiles.is_empty()
        || !snapshots.tools.is_empty()
    {
        return Err(AppError::BadRequest(
            "Workflow dependency authority is not available".into(),
        ));
    }
    for child in bundle.children().values() {
        supported_publication(child)?;
    }
    Ok(())
}
