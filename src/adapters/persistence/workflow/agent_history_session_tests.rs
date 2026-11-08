use super::*;
use crate::application::workflow::agent_history::AgentHistorySessionRef;

async fn session(f: &AdmissionFixture, run: RunId) -> Uuid {
    sqlx::query_scalar(
        "SELECT session_id FROM workflow_agent_sessions WHERE company_id=$1 AND run_id=$2",
    )
    .bind(f.binding.target.company.as_uuid())
    .bind(run.as_uuid())
    .fetch_one(f.persistence().pool())
    .await
    .unwrap()
}
async fn admitted(f: &AdmissionFixture, key: &str, reuse: Option<RunId>) -> PreparedAdmission {
    let mut request = f.request(key, f.manual());
    request.history_session = reuse.map(|from_run| AgentHistorySessionRef { from_run });
    let command = f.prepare(request).await;
    assert!(matches!(
        f.persistence().admit(&command).await.unwrap(),
        AdmissionResult::Created(_)
    ));
    command
}

#[tokio::test]
async fn agent_history_identity_private_fresh_reuse_and_replay_are_admission_owned() {
    let f = AdmissionFixture::new().await;
    let first = admitted(&f, "first", None).await;
    let second = admitted(&f, "fresh", None).await;
    let reused = admitted(&f, "reused", Some(first.proposed_run_id())).await;
    let original = session(&f, first.proposed_run_id()).await;
    assert_ne!(original, session(&f, second.proposed_run_id()).await);
    assert_eq!(original, session(&f, reused.proposed_run_id()).await);
    assert!(matches!(
        f.persistence().admit(&reused).await.unwrap(),
        AdmissionResult::Replayed(_)
    ));
    let mut request = f.request("reused", reused.trigger().clone());
    request.history_session = Some(AgentHistorySessionRef {
        from_run: second.proposed_run_id(),
    });
    assert!(matches!(
        f.persistence()
            .admit(&f.prepare(request).await)
            .await
            .unwrap(),
        AdmissionResult::Conflict
    ));
    // Absence retains normal admission replay equivalence and the already committed mapping.
    let request = f.request("reused", reused.trigger().clone());
    assert!(matches!(
        f.persistence()
            .admit(&f.prepare(request).await)
            .await
            .unwrap(),
        AdmissionResult::Replayed(_)
    ));
    let counts:(i64,i64)=sqlx::query_as("SELECT (SELECT count(*) FROM agent_history_sessions WHERE company_id=$1),(SELECT count(*) FROM workflow_agent_sessions WHERE company_id=$1)")
        .bind(f.binding.target.company.as_uuid()).fetch_one(f.persistence().pool()).await.unwrap();
    assert_eq!(counts, (2, 3));
}

#[tokio::test]
async fn agent_history_identity_competing_admissions_allocate_one_private_session() {
    let f = AdmissionFixture::new().await;
    let command = f.prepare(f.request("competing-session", f.manual())).await;
    let (a, b) = tokio::join!(
        f.persistence().admit(&command),
        f.persistence().admit(&command)
    );
    assert!(matches!(
        (a.unwrap(), b.unwrap()),
        (AdmissionResult::Created(_), AdmissionResult::Replayed(_))
            | (AdmissionResult::Replayed(_), AdmissionResult::Created(_))
    ));
    assert_eq!(
        session(&f, command.proposed_run_id()).await,
        command.proposed_run_id().as_uuid()
    );
    let count: i64 =
        sqlx::query_scalar("SELECT count(*) FROM agent_history_sessions WHERE company_id=$1")
            .bind(f.binding.target.company.as_uuid())
            .fetch_one(f.persistence().pool())
            .await
            .unwrap();
    assert_eq!(count, 1);
}

#[tokio::test]
async fn agent_history_identity_manual_starter_owns_session_and_ceiling_not_binding_actor() {
    let f = AdmissionFixture::new().await;
    let original = admitted(&f, "owner", None).await;
    let admin = super::super::message_tests::support::member(&f.binding.fixture, "admin").await;
    sqlx::query("UPDATE company_members SET access_level=5 WHERE company_id=$1 AND user_id=$2")
        .bind(f.binding.target.company.as_uuid())
        .bind(admin)
        .execute(f.persistence().pool())
        .await
        .unwrap();
    let mut request = f.request("actual-starter", f.manual());
    request.actor = WorkflowActor::authenticated(admin).unwrap();
    let command = f.prepare(request).await;
    f.persistence().admit(&command).await.unwrap();
    #[derive(Debug, PartialEq, Eq, sqlx::FromRow)]
    struct Starter {
        actor_id: Uuid,
        creator_user_id: Uuid,
        knowledge_ceiling: i16,
    }
    let saved=sqlx::query_as::<_,Starter>("SELECT run.actor_id,session.creator_user_id,run.knowledge_ceiling FROM workflow_runs AS run JOIN workflow_agent_sessions AS mapping ON mapping.company_id=run.company_id AND mapping.run_id=run.id JOIN agent_history_sessions AS session ON session.company_id=mapping.company_id AND session.id=mapping.session_id WHERE run.company_id=$1 AND run.id=$2")
        .bind(f.binding.target.company.as_uuid()).bind(command.proposed_run_id().as_uuid()).fetch_one(f.persistence().pool()).await.unwrap();
    assert_eq!(
        saved,
        Starter {
            actor_id: admin,
            creator_user_id: admin,
            knowledge_ceiling: 5
        }
    );
    let mut refused = f.request("foreign-creator", f.manual());
    refused.actor = WorkflowActor::authenticated(admin).unwrap();
    refused.history_session = Some(AgentHistorySessionRef {
        from_run: original.proposed_run_id(),
    });
    assert!(matches!(
        f.persistence().admit(&f.prepare(refused).await).await,
        Err(AppError::NotFound(_))
    ));
    let member = super::super::message_tests::support::member(&f.binding.fixture, "member").await;
    let mut refused = f.request("member-refused", f.manual());
    refused.actor = WorkflowActor::authenticated(member).unwrap();
    assert!(matches!(
        f.persistence().admit(&f.prepare(refused).await).await,
        Err(AppError::NotFound(_))
    ));
}

#[tokio::test]
async fn agent_history_identity_native_mapping_and_creator_are_immutable() {
    let f = AdmissionFixture::new().await;
    let first = admitted(&f, "native-first", None).await;
    let second = admitted(&f, "native-second", None).await;
    let p = f.persistence().pool();
    for statement in [
        "UPDATE workflow_agent_sessions SET session_id=$3 WHERE company_id=$1 AND run_id=$2",
        "UPDATE agent_history_sessions SET creator_user_id=$3 WHERE company_id=$1 AND id=$2",
        "DELETE FROM workflow_agent_sessions WHERE company_id=$1 AND run_id=$2 AND session_id<>$3",
    ] {
        let error = sqlx::query(statement)
            .bind(f.binding.target.company.as_uuid())
            .bind(first.proposed_run_id().as_uuid())
            .bind(second.proposed_run_id().as_uuid())
            .execute(p)
            .await
            .unwrap_err();
        assert_eq!(
            error
                .as_database_error()
                .and_then(|error| error.code())
                .as_deref(),
            Some("23514")
        );
    }
    let mismatch=sqlx::query("INSERT INTO agent_history_sessions (company_id,id,creator_principal_id,creator_user_id) SELECT company_id,$2,creator_principal_id,creator_user_id FROM agent_history_sessions WHERE company_id=$1 LIMIT 1")
        .bind(f.binding.target.company.as_uuid()).bind(Uuid::new_v4()).execute(p).await.unwrap_err();
    assert_eq!(
        mismatch
            .as_database_error()
            .and_then(|error| error.code())
            .as_deref(),
        Some("23514")
    );
}

#[tokio::test]
async fn agent_history_identity_foreign_and_changed_association_reuse_refuse_without_writes() {
    let f = AdmissionFixture::new().await;
    let first = admitted(&f, "private-original", None).await;
    let foreign = f.foreign().await;
    let other = admitted(&foreign, "foreign-original", None).await;
    let conversation = super::admission_history_tests::Conversation::new(&f).await;
    for (index, from_run) in [
        other.proposed_run_id(),
        RunId::new(Uuid::new_v4()),
        first.proposed_run_id(),
    ]
    .into_iter()
    .enumerate()
    {
        let mut request = f.request(&format!("private-refusal-{index}"), f.manual());
        request.history_session = Some(AgentHistorySessionRef { from_run });
        if index == 2 {
            request.association = RelatedAssociation::Thread {
                channel_id: RelatedChannelId::new(conversation.channel),
                thread_id: RelatedThreadId::new(conversation.thread),
            };
        }
        let command = f.prepare(request).await;
        assert!(matches!(
            f.persistence().admit(&command).await,
            Err(AppError::NotFound(_))
        ));
    }
    let count: i64 =
        sqlx::query_scalar("SELECT count(*) FROM workflow_agent_sessions WHERE company_id=$1")
            .bind(f.binding.target.company.as_uuid())
            .fetch_one(f.persistence().pool())
            .await
            .unwrap();
    assert_eq!(count, 1);
}

#[tokio::test]
async fn agent_history_identity_automatic_producer_cannot_supply_private_session_reference() {
    let f = AdmissionFixture::new().await;
    let original = admitted(&f, "original", None).await;
    let conversation = super::admission_history_tests::Conversation::new(&f).await;
    let mut request = conversation.request(&f, "forbidden-reuse", Uuid::new_v4());
    request.history_session = Some(AgentHistorySessionRef {
        from_run: original.proposed_run_id(),
    });
    let result = WorkflowService::new(
        f.persistence().clone(),
        Capture::default(),
        NoRuntime,
        NoRuntime,
        Preflight,
    )
    .admit(request)
    .await;
    assert!(matches!(result, Err(AppError::BadRequest(_))));
}
