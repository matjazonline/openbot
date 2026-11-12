use super::*;

/// Company/member/principal locks are acquired by authorize_company first.
/// Channel settings writers update the channel before replacing grants; direct
/// grant revocation conflicts with the granted row's SHARE lock below.
pub(in crate::adapters::persistence) async fn authorize_association(
    tx: &mut PgConnection,
    company: CompanyId,
    actor: WorkflowActor,
    association: RelatedAssociation,
) -> AppResult<()> {
    authorize_association_with_locks(
        tx,
        company,
        actor,
        association,
        super::row_locks::RowLocks::Wait,
    )
    .await
}

pub(in crate::adapters::persistence) async fn authorize_association_with_locks(
    tx: &mut PgConnection,
    company: CompanyId,
    actor: WorkflowActor,
    association: RelatedAssociation,
    locks: super::row_locks::RowLocks,
) -> AppResult<()> {
    let channel = match association {
        RelatedAssociation::Company => return Ok(()),
        RelatedAssociation::Channel(channel)
        | RelatedAssociation::Thread {
            channel_id: channel,
            ..
        } => channel,
    };
    let mode: String = sqlx::query_scalar(
        &locks.sql("SELECT access_mode FROM channels WHERE company_id = $1 AND id = $2 FOR SHARE"),
    )
    .bind(company.as_uuid())
    .bind(channel.as_uuid())
    .fetch_optional(&mut *tx)
    .await?
    .ok_or_else(missing)?;
    let owner: bool = sqlx::query_scalar("SELECT user_id = $2 FROM companies WHERE id = $1")
        .bind(company.as_uuid())
        .bind(actor.user_id())
        .fetch_one(&mut *tx)
        .await?;
    match mode.as_str() {
        "team" | "public" => {}
        "allowlist" if owner => {}
        "allowlist" => {
            sqlx::query_scalar::<_, Uuid>(&locks.sql(
                "SELECT access_grant.principal_id FROM channel_principal_grants AS access_grant \
                 JOIN principals AS principal ON principal.company_id = access_grant.company_id \
                   AND principal.id = access_grant.principal_id \
                 JOIN users AS account ON account.id = principal.user_id \
                 WHERE access_grant.company_id = $1 AND access_grant.channel_id = $2 \
                   AND principal.user_id = $3 AND account.email_verified_at IS NOT NULL \
                   AND access_grant.capability = 'view' \
                 FOR SHARE OF access_grant, account",
            ))
            .bind(company.as_uuid())
            .bind(channel.as_uuid())
            .bind(actor.user_id())
            .fetch_optional(&mut *tx)
            .await?
            .ok_or_else(missing)?;
        }
        _ => return Err(invalid()),
    }
    if let RelatedAssociation::Thread { thread_id, .. } = association {
        sqlx::query_scalar::<_, Uuid>(
            &locks.sql("SELECT id FROM threads WHERE company_id = $1 AND channel_id = $2 AND id = $3 FOR SHARE"),
        ).bind(company.as_uuid()).bind(channel.as_uuid()).bind(thread_id.as_uuid())
            .fetch_optional(&mut *tx).await?.ok_or_else(missing)?;
    }
    Ok(())
}
