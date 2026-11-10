//! Current reader facts and durable run classification. All state changes use the caller's transaction.
use super::*;
use crate::adapters::persistence::participant::{membership_from_db, reader_level};
use crate::entities::{
    access_level::{
        AccessLevel, ExactnessTarget, HumanInputClass, RunKnowledge, audience_level,
        human_input_exactness,
    },
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
/// A new root run's levels and whether its level is exact (phase 05 E3). Manual input is
/// conversational; schedule params are authored configuration; a trigger message is exact
/// only when an agent or system wrote it under an exact run.
pub(super) struct AdmittedKnowledge {
    pub levels: RunKnowledge,
    pub exact: bool,
}

#[derive(sqlx::FromRow)]
struct TriggerStamp {
    access_level: i16,
    role: String,
    content_exact: Option<bool>,
}

pub(super) async fn admission_levels_on(
    db: &mut PgConnection,
    command: &PreparedAdmission,
) -> AppResult<AdmittedKnowledge> {
    let company = command.company_id();
    let (levels, exact) = match command.trigger().source() {
        TriggerSource::Manual => (
            RunKnowledge::new(
                actor_level_on(db, company, Some(command.actor().user_id())).await?,
                AccessLevel::PUBLIC,
            ),
            human_input_exactness(HumanInputClass::ConversationalInput, ExactnessTarget::Run),
        ),
        TriggerSource::Schedule { .. } => (
            RunKnowledge::new(
                actor_level_on(db, company, Some(command.binding().run_as().user_id())).await?,
                AccessLevel::PUBLIC,
            ),
            human_input_exactness(HumanInputClass::AuthoredConfiguration, ExactnessTarget::Run),
        ),
        TriggerSource::Message { message_id } => {
            let ceiling = thread_audience_on(db, company, command.association()).await?;
            let stamp = sqlx::query_as::<_, TriggerStamp>(
                "SELECT access_level::smallint AS access_level,role,content_exact FROM messages \
                 WHERE company_id=$1 AND id=$2",
            )
            .bind(company.as_uuid())
            .bind(message_id.as_uuid())
            .fetch_optional(&mut *db)
            .await?
            .ok_or_else(missing)?;
            let content = AccessLevel::try_from(stamp.access_level)
                .map_err(|e| AppError::Internal(e.into()))?;
            if content > ceiling {
                return Err(AppError::KnowledgeTrigger { content, ceiling });
            }
            let exact = stored_message_exactness(&stamp.role, stamp.content_exact)?;
            (RunKnowledge::new(ceiling, content), exact)
        }
        TriggerSource::Child { .. } => {
            return Err(AppError::BadRequest(
                "Child runs inherit their parent's knowledge facts".into(),
            ));
        }
    };
    Ok(AdmittedKnowledge {
        levels: levels.map_err(|e| AppError::Internal(e.into()))?,
        exact,
    })
}

/// A stored message's exactness: human rows carry none and are conversational input; agent
/// and system rows carry the producing run's bit. Any other shape is malformed.
pub(super) fn stored_message_exactness(role: &str, stored: Option<bool>) -> AppResult<bool> {
    let malformed = || AppError::Internal("Malformed stored message exactness".into());
    match (role, stored) {
        ("human", None) => Ok(human_input_exactness(
            HumanInputClass::ConversationalInput,
            ExactnessTarget::Run,
        )),
        ("agent" | "system", Some(exact)) => Ok(exact),
        _ => Err(malformed()),
    }
}

/// Record that the run consumed content its level does not account for. The caller holds the
/// run's live fence/run lock, as for [`raise_content_level_on`]. Exactness is only lowered.
pub(crate) async fn lower_content_exact_on(
    db: &mut PgConnection,
    company: CompanyId,
    run: RunId,
) -> AppResult<()> {
    let changed =
        sqlx::query("UPDATE workflow_runs SET content_exact=false WHERE company_id=$1 AND id=$2")
            .bind(company.as_uuid())
            .bind(run.as_uuid())
            .execute(db)
            .await?;
    if changed.rows_affected() != 1 {
        return Err(missing());
    }
    Ok(())
}

/// Absorb consumed content: raise the level and, when the content was inexact, lower the bit.
pub(crate) async fn absorb_content_on(
    db: &mut PgConnection,
    company: CompanyId,
    run: RunId,
    level: AccessLevel,
    exact: bool,
) -> AppResult<()> {
    raise_content_level_on(db, company, run, level).await?;
    if !exact {
        lower_content_exact_on(db, company, run).await?;
    }
    Ok(())
}

/// The run's current destination audience (E3): a thread's lowest human reader, otherwise
/// the run actor's clearance. One helper for every memory read and save.
pub(crate) async fn destination_level_on(
    db: &mut PgConnection,
    company: CompanyId,
    run: RunId,
) -> AppResult<AccessLevel> {
    let (actor, channel, thread): (Uuid, Option<Uuid>, Option<Uuid>) = sqlx::query_as(
        "SELECT run.actor_id, run.channel_id, run.thread_id FROM workflow_runs AS run \
         WHERE run.company_id=$1 AND run.id=$2",
    )
    .bind(company.as_uuid())
    .bind(run.as_uuid())
    .fetch_optional(&mut *db)
    .await?
    .ok_or_else(missing)?;
    let association = binding_rows::association_from_columns(channel, thread)?;
    match association {
        RelatedAssociation::Thread { .. } => thread_audience_on(db, company, association).await,
        _ => actor_level_on(db, company, Some(actor)).await,
    }
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

/// Test fixtures for exactness (phase 05 E3). Production never raises `content_exact`; most
/// fixtures admit manual runs, which start inexact, so the hook tests first mark a run exact
/// with triggers disabled for that one transaction (the precedent in `child_output_tests`).
#[cfg(test)]
pub(crate) mod exactness_fixture {
    use super::*;
    pub(crate) async fn mark_exact(pool: &sqlx::PgPool, company: CompanyId, run: RunId) {
        let mut tx = pool.begin().await.unwrap();
        sqlx::query("SET LOCAL session_replication_role = replica")
            .execute(&mut *tx)
            .await
            .unwrap();
        let changed = sqlx::query(
            "UPDATE workflow_runs SET content_exact=true WHERE company_id=$1 AND id=$2",
        )
        .bind(company.as_uuid())
        .bind(run.as_uuid())
        .execute(&mut *tx)
        .await
        .unwrap();
        assert_eq!(changed.rows_affected(), 1);
        tx.commit().await.unwrap();
    }
    pub(crate) async fn exact(pool: &sqlx::PgPool, company: CompanyId, run: RunId) -> bool {
        sqlx::query_scalar("SELECT content_exact FROM workflow_runs WHERE company_id=$1 AND id=$2")
            .bind(company.as_uuid())
            .bind(run.as_uuid())
            .fetch_one(pool)
            .await
            .unwrap()
    }
}
