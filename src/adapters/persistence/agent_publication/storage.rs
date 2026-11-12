use super::*;
use sha2::{Digest, Sha256};

#[derive(sqlx::FromRow)]
struct RevisionRow {
    id: Uuid,
    draft_revision: i64,
    content: String,
    digest: Vec<u8>,
}

pub(in crate::adapters::persistence) async fn active_on(
    db: &mut PgConnection,
    scope: AgentPublicationScope,
) -> AppResult<Option<AgentRevisionId>> {
    let id = sqlx::query_scalar::<_, Uuid>(
        "SELECT revision_id FROM agent_active_revisions WHERE company_id=$1 AND agent_id=$2 FOR SHARE"
    ).bind(scope.company.as_uuid()).bind(scope.agent.as_uuid()).fetch_optional(db).await?;
    Ok(id.map(AgentRevisionId::new))
}

pub(in crate::adapters::persistence) async fn revision_on(
    db: &mut PgConnection,
    scope: AgentPublicationScope,
    id: AgentRevisionId,
) -> AppResult<Option<PublishedAgentRevision>> {
    revision_with_locks_on(
        db,
        scope,
        id,
        super::super::workflow::row_locks::RowLocks::Wait,
    )
    .await
}

pub(in crate::adapters::persistence) async fn revision_with_locks_on(
    db: &mut PgConnection,
    scope: AgentPublicationScope,
    id: AgentRevisionId,
    locks: super::super::workflow::row_locks::RowLocks,
) -> AppResult<Option<PublishedAgentRevision>> {
    let row = sqlx::query_as::<_, RevisionRow>(&locks.sql(
        "SELECT id,draft_revision,content,digest FROM agent_revisions \
         WHERE company_id=$1 AND agent_id=$2 AND id=$3 FOR SHARE",
    ))
    .bind(scope.company.as_uuid())
    .bind(scope.agent.as_uuid())
    .bind(id.as_uuid())
    .fetch_optional(db)
    .await?;
    row.map(|row| decode_revision(scope, row)).transpose()
}

fn decode_revision(
    scope: AgentPublicationScope,
    row: RevisionRow,
) -> AppResult<PublishedAgentRevision> {
    let definition = ResolvedAgentDefinition::decode(row.content.as_bytes())?;
    let digest: [u8; 32] = Sha256::digest(row.content.as_bytes()).into();
    if definition.company_id != scope.company
        || definition.agent_id != scope.agent
        || row.digest.as_slice() != digest
        || definition.canonical_content()? != row.content.as_bytes()
    {
        return Err(invalid());
    }
    Ok(PublishedAgentRevision {
        id: AgentRevisionId::new(row.id),
        draft_revision: AgentDraftRevision::new(
            u64::try_from(row.draft_revision).map_err(|_| invalid())?,
        )
        .map_err(|_| invalid())?,
        definition,
        digest,
    })
}

#[derive(sqlx::FromRow)]
struct RequestRow {
    expected_draft_revision: i64,
    expected_active_revision: Option<Uuid>,
    result_revision: Uuid,
}

pub(super) async fn replay_on(
    db: &mut PgConnection,
    request: &PublishAgent,
) -> AppResult<Option<PublishedAgentRevision>> {
    let row = sqlx::query_as::<_, RequestRow>(
        "SELECT expected_draft_revision,expected_active_revision,result_revision \
         FROM agent_publication_requests WHERE company_id=$1 AND agent_id=$2 AND key=$3 FOR SHARE",
    )
    .bind(request.scope.company.as_uuid())
    .bind(request.scope.agent.as_uuid())
    .bind(request.key.as_str())
    .fetch_optional(&mut *db)
    .await?;
    let Some(row) = row else {
        return Ok(None);
    };
    if row.expected_draft_revision != request.expected_draft_revision.get() as i64
        || row.expected_active_revision != request.expected_active_revision.map(|id| id.as_uuid())
    {
        return Err(conflict());
    }
    revision_on(db, request.scope, AgentRevisionId::new(row.result_revision))
        .await?
        .map(Some)
        .ok_or_else(invalid)
}

pub(super) async fn insert_revision_on(
    db: &mut PgConnection,
    scope: AgentPublicationScope,
    draft: AgentDraftRevision,
    definition: ResolvedAgentDefinition,
) -> AppResult<PublishedAgentRevision> {
    let id = AgentRevisionId::new(Uuid::new_v4());
    let bytes = definition.canonical_content()?;
    let digest: [u8; 32] = Sha256::digest(&bytes).into();
    let content = String::from_utf8(bytes).map_err(|_| invalid())?;
    sqlx::query(
        "INSERT INTO agent_revisions(company_id,agent_id,id,draft_revision,content,digest) VALUES($1,$2,$3,$4,$5,$6)"
    ).bind(scope.company.as_uuid()).bind(scope.agent.as_uuid()).bind(id.as_uuid())
        .bind(draft.get() as i64).bind(content).bind(digest.as_slice()).execute(&mut *db).await?;
    for (position, tool) in definition.tools.iter().enumerate() {
        sqlx::query(
            "INSERT INTO agent_revision_bindings(company_id,agent_id,revision_id,binding_id,position) VALUES($1,$2,$3,$4,$5)"
        ).bind(scope.company.as_uuid()).bind(scope.agent.as_uuid()).bind(id.as_uuid())
            .bind(tool.binding.id.as_uuid()).bind(i16::try_from(position).map_err(|_| invalid())?)
            .execute(&mut *db).await?;
    }
    for (position, assigned) in definition.skills.iter().enumerate() {
        sqlx::query(
            "INSERT INTO agent_revision_skills(company_id,agent_id,revision_id,assignment_id,skill_id,position) VALUES($1,$2,$3,$4,$5,$6)"
        ).bind(scope.company.as_uuid()).bind(scope.agent.as_uuid()).bind(id.as_uuid())
            .bind(assigned.assignment_id.as_uuid()).bind(assigned.skill.skill_id)
            .bind(i16::try_from(position).map_err(|_| invalid())?).execute(&mut *db).await?;
    }
    Ok(PublishedAgentRevision {
        id,
        draft_revision: draft,
        definition,
        digest,
    })
}

pub(super) async fn activate_on(
    db: &mut PgConnection,
    request: &PublishAgent,
    result: &PublishedAgentRevision,
) -> AppResult<()> {
    super::grants::activate_on(db, request.scope, &result.definition).await?;
    sqlx::query(
        "INSERT INTO agent_active_revisions(company_id,agent_id,revision_id) VALUES($1,$2,$3) \
         ON CONFLICT(company_id,agent_id) DO UPDATE SET revision_id=EXCLUDED.revision_id",
    )
    .bind(request.scope.company.as_uuid())
    .bind(request.scope.agent.as_uuid())
    .bind(result.id.as_uuid())
    .execute(&mut *db)
    .await?;
    sqlx::query(
        "INSERT INTO agent_publication_requests(company_id,agent_id,key,expected_draft_revision,expected_active_revision,result_revision) \
         VALUES($1,$2,$3,$4,$5,$6)"
    ).bind(request.scope.company.as_uuid()).bind(request.scope.agent.as_uuid()).bind(request.key.as_str())
        .bind(request.expected_draft_revision.get() as i64)
        .bind(request.expected_active_revision.map(|id| id.as_uuid())).bind(result.id.as_uuid())
        .execute(db).await?;
    Ok(())
}
