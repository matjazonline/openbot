//! Current reader facts and durable run classification. All state changes use the caller's transaction.
use super::*;
use crate::adapters::persistence::participant::{membership_from_db, reader_level};
use crate::entities::{
    access_level::{AccessLevel, RunKnowledge, audience_level},
    participant::PrincipalKind,
};
use std::str::FromStr;

const MAX_READER_FACTS: usize = 64;
#[derive(sqlx::FromRow)]
pub(super) struct ReaderFact {
    pub(super) kind: String,
    pub(super) membership: Option<String>,
    pub(super) access_level: Option<i16>,
    pub(super) is_owner: bool,
}
pub(super) fn reader(company: CompanyId, fact: ReaderFact) -> AppResult<Option<AccessLevel>> {
    let kind =
        PrincipalKind::from_str(&fact.kind).map_err(|e| AppError::Internal(e.to_string()))?;
    let membership = membership_from_db(fact.membership.as_deref())?;
    if fact.is_owner != membership.is_owner() {
        return Err(AppError::Internal(
            "Inconsistent stored reader ownership".into(),
        ));
    }
    match kind {
        PrincipalKind::Agent | PrincipalKind::System => Ok(None),
        PrincipalKind::Person | PrincipalKind::External => {
            reader_level(company.as_uuid(), membership, fact.access_level).map(Some)
        }
    }
}
fn audience(company: CompanyId, facts: Vec<ReaderFact>) -> AppResult<AccessLevel> {
    if facts.len() > MAX_READER_FACTS {
        return Err(AppError::Internal(
            "Too many distinct stored reader facts".into(),
        ));
    }
    let readers = facts
        .into_iter()
        .map(|fact| reader(company, fact))
        .collect::<AppResult<Vec<_>>>()?;
    Ok(audience_level(readers.into_iter().flatten()))
}
pub(super) async fn thread_audience_on(
    db: &mut PgConnection,
    company: CompanyId,
    association: RelatedAssociation,
) -> AppResult<AccessLevel> {
    let RelatedAssociation::Thread {
        channel_id,
        thread_id,
    } = association
    else {
        return Ok(AccessLevel::PUBLIC);
    };
    // One MVCC statement is the audience's freeze point. DISTINCT bounds allocation by
    // finite kind/role/level combinations, rather than the size of a thread's party list.
    let facts=sqlx::query_as::<_,ReaderFact>(
        "SELECT DISTINCT principal.kind,member.role AS membership,member.access_level::smallint AS access_level, \
            COALESCE(company.user_id=principal.user_id,false) AS is_owner \
         FROM thread_principals AS party JOIN principals AS principal \
           ON (principal.company_id,principal.id)=(party.company_id,party.principal_id) \
         JOIN companies AS company ON company.id=party.company_id \
         LEFT JOIN company_members AS member ON (member.company_id,member.user_id)=(principal.company_id,principal.user_id) \
         WHERE party.company_id=$1 AND party.channel_id=$2 AND party.thread_id=$3 LIMIT $4")
        .bind(company.as_uuid()).bind(channel_id.as_uuid()).bind(thread_id.as_uuid()).bind((MAX_READER_FACTS+1) as i64)
        .fetch_all(db).await?;
    audience(company, facts)
}
pub(super) async fn actor_level_on(
    db: &mut PgConnection,
    company: CompanyId,
    actor: Option<Uuid>,
) -> AppResult<AccessLevel> {
    let Some(actor) = actor else {
        return Ok(AccessLevel::PUBLIC);
    };
    let fact=sqlx::query_as::<_,ReaderFact>(
        "SELECT principal.kind,member.role AS membership,member.access_level::smallint AS access_level, \
            company.user_id=member.user_id AS is_owner \
         FROM company_members AS member JOIN companies AS company ON company.id=member.company_id \
         JOIN principals AS principal ON (principal.company_id,principal.user_id)=(member.company_id,member.user_id) \
         WHERE member.company_id=$1 AND member.user_id=$2")
        .bind(company.as_uuid()).bind(actor).fetch_optional(db).await?;
    match fact {
        Some(fact) => reader(company, fact)?
            .ok_or_else(|| AppError::Internal("A workflow actor is not a person".into())),
        None => Ok(AccessLevel::PUBLIC),
    }
}
pub(super) fn stored_levels(ceiling: i16, content: i16) -> AppResult<RunKnowledge> {
    let ceiling = AccessLevel::try_from(ceiling).map_err(|e| AppError::Internal(e.into()))?;
    let content = AccessLevel::try_from(content).map_err(|e| AppError::Internal(e.into()))?;
    RunKnowledge::new(ceiling, content).map_err(|e| AppError::Internal(e.into()))
}
pub(super) async fn admission_levels_on(
    db: &mut PgConnection,
    command: &PreparedAdmission,
) -> AppResult<RunKnowledge> {
    let company = command.company_id();
    let levels = match command.trigger().source() {
        TriggerSource::Manual => RunKnowledge::new(
            actor_level_on(db, company, Some(command.actor().user_id())).await?,
            AccessLevel::PUBLIC,
        ),
        TriggerSource::Schedule { .. } => RunKnowledge::new(
            actor_level_on(db, company, Some(command.binding().run_as().user_id())).await?,
            AccessLevel::PUBLIC,
        ),
        TriggerSource::Message { message_id } => {
            let ceiling = thread_audience_on(db, company, command.association()).await?;
            let stamp: i16 = sqlx::query_scalar(
                "SELECT access_level::smallint FROM messages WHERE company_id=$1 AND id=$2",
            )
            .bind(company.as_uuid())
            .bind(message_id.as_uuid())
            .fetch_optional(&mut *db)
            .await?
            .ok_or_else(missing)?;
            let content = AccessLevel::try_from(stamp).map_err(|e| AppError::Internal(e.into()))?;
            if content > ceiling {
                return Err(AppError::KnowledgeTrigger { content, ceiling });
            }
            RunKnowledge::new(ceiling, content)
        }
        TriggerSource::Child { .. } => {
            return Err(AppError::BadRequest(
                "Child runs inherit their parent's knowledge facts".into(),
            ));
        }
    };
    levels.map_err(|e| AppError::Internal(e.into()))
}

/// Commit a read's classification with its output. The caller must already hold its live
/// workflow fence/run lock; this is an internal primitive, never a standalone write endpoint.
pub(crate) async fn raise_content_level_on(
    db: &mut PgConnection,
    company: CompanyId,
    run: RunId,
    level: AccessLevel,
) -> AppResult<()> {
    let changed = sqlx::query(
        "UPDATE workflow_runs SET content_level=GREATEST(content_level,$3::smallint) \
         WHERE company_id=$1 AND id=$2",
    )
    .bind(company.as_uuid())
    .bind(run.as_uuid())
    .bind(level.as_i16())
    .execute(db)
    .await?;
    if changed.rows_affected() != 1 {
        return Err(missing());
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    fn fact(kind: &str) -> ReaderFact {
        ReaderFact {
            kind: kind.into(),
            membership: None,
            access_level: None,
            is_owner: false,
        }
    }
    #[test]
    fn knowledge_run_level_reader_facts_exclude_nonreaders_and_reject_malformed_authority() {
        let company = CompanyId::new(Uuid::new_v4());
        assert_eq!(
            audience(company, vec![fact("agent"), fact("system")]).unwrap(),
            AccessLevel::PUBLIC
        );
        assert_eq!(
            reader(company, fact("external")).unwrap(),
            Some(AccessLevel::PUBLIC)
        );
        assert!(reader(company, fact("unknown")).is_err());
        assert!(
            reader(
                company,
                ReaderFact {
                    membership: Some("owner".into()),
                    access_level: Some(9),
                    ..fact("person")
                }
            )
            .is_err()
        );
        assert!(
            reader(
                company,
                ReaderFact {
                    membership: Some("admin".into()),
                    access_level: None,
                    ..fact("person")
                }
            )
            .is_err()
        );
        assert!(
            reader(
                company,
                ReaderFact {
                    membership: Some("admin".into()),
                    access_level: Some(10),
                    ..fact("person")
                }
            )
            .is_err()
        );
    }
}
