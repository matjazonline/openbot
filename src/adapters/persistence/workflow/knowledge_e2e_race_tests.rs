//! Observe the actual private tool write stalled in PostgreSQL, then commit an audience change.
use super::*;
use crate::adapters::persistence::workflow::tests::admission_tests::lease_tests::resolution_gate::{Gate, Stall, connect};
use crate::application::attention::AttentionPersistence;
use crate::entities::attention::{AttentionQuery, AttentionSourceKind, AttentionView};
use crate::entities::transport::PrincipalId;

enum AudienceChange {
    CustomerCc,
    MemberDemotion,
}

async fn change_audience(j: &SalesJourney, member: Uuid, thread: Uuid, change: AudienceChange) {
    let company = j.d.fixture.target.company.as_uuid();
    match change {
        AudienceChange::CustomerCc => {
            let affected = sqlx::query("INSERT INTO thread_principals(company_id,channel_id,thread_id,principal_id,role)
                SELECT $1,$2,$3,identity.principal_id,'participant' FROM participant_identities AS identity
                WHERE identity.company_id=$1 AND identity.transport='email' AND identity.subject=$4")
                .bind(company).bind(j.d.channel).bind(thread).bind(CUSTOMER)
                .execute(j.d.p().pool()).await.unwrap().rows_affected();
            assert_eq!(
                affected, 1,
                "one actual external CC identity joined the conversation"
            );
        }
        AudienceChange::MemberDemotion => {
            let affected = sqlx::query("UPDATE company_members SET access_level=0 WHERE company_id=$1 AND user_id=$2 AND access_level=5")
                .bind(company).bind(member).execute(j.d.p().pool()).await.unwrap().rows_affected();
            assert_eq!(affected, 1, "actual current recipient was lowered");
        }
    }
}

async fn assert_typed_attention(j: &SalesJourney) {
    let company = j.d.fixture.target.company.as_uuid();
    let run: Uuid = sqlx::query_scalar("SELECT id FROM workflow_runs WHERE company_id=$1")
        .bind(company)
        .fetch_one(j.d.p().pool())
        .await
        .unwrap();
    let code: String = sqlx::query_scalar("SELECT attempt.workflow_failure_code FROM task_attempts AS attempt
        JOIN background_tasks AS job ON job.id=attempt.task_id
        JOIN workflow_executions AS execution ON (execution.company_id,execution.id)=(job.company_id,job.workflow_execution_id)
        WHERE execution.company_id=$1 AND execution.run_id=$2 AND attempt.workflow_failure_code IS NOT NULL")
        .bind(company).bind(run).fetch_one(j.d.p().pool()).await.unwrap();
    assert_eq!(code, crate::app_error::KNOWLEDGE_AUDIENCE_BELOW_CONTENT);
    let owner: Uuid =
        sqlx::query_scalar("SELECT id FROM principals WHERE company_id=$1 AND user_id=$2")
            .bind(company)
            .bind(j.d.fixture.target.actor.user_id())
            .fetch_one(j.d.p().pool())
            .await
            .unwrap();
    let attention =
        j.d.p()
            .list_attention(AttentionQuery {
                company_id: company,
                principal_id: PrincipalId::new(owner),
                visible_channel_ids: &[j.d.channel],
                manager: true,
                view: AttentionView::MyWork,
                cursor: None,
                limit: 100,
            })
            .await
            .unwrap();
    assert!(attention.items.iter().any(
        |item| item.source_kind == AttentionSourceKind::RunFailure && item.run_id == Some(run)
    ));
}

#[tokio::test]
async fn knowledge_e2e_cc_and_member_demotion_mid_reply_refuse_delivery_and_show_typed_attention() {
    for change in [AudienceChange::CustomerCc, AudienceChange::MemberDemotion] {
        // Only the initial search call is allowed. The scripted server rejects any
        // second request, independently of the recorded request-content assertions.
        let expected = turns("sales.md: PRIVATE FLOOR 60")
            .into_iter()
            .take(1)
            .collect();
        let mut j = Box::pin(SalesJourney::new(expected, Audience::Team)).await;
        let member = j.member(AccessLevel::L5).await;
        j.d.bind(answering_source(), Some(MessageTrigger::Direct))
            .await;
        let inbound =
            j.d.email(email(&j.d, member.email.as_str(), "race-sales"))
                .await;
        let thread = inbound.thread.unwrap().id;
        let (mut observer, _) = connect(&j).await;
        let mut gate = Gate::arm(
            &j,
            "workflow_knowledge_consumptions",
            "INSERT",
            "true",
            Stall::Statement {
                table: "workflow_knowledge_consumptions",
                events: "INSERT",
                when: "true",
            },
        )
        .await;
        {
            let drive = j.drive(1);
            tokio::pin!(drive);
            tokio::select! {
                _ = gate.stalled(&mut observer, &[&||true]) => {},
                _ = drive.as_mut() => panic!("workflow completed before its actual observation gate"),
            }
            let ceiling: i16 = sqlx::query_scalar(
                "SELECT knowledge_ceiling::smallint FROM workflow_runs WHERE company_id=$1",
            )
            .bind(j.d.fixture.target.company.as_uuid())
            .fetch_one(j.d.p().pool())
            .await
            .unwrap();
            assert_eq!(
                ceiling, 5,
                "admission preceded the actual audience mutation"
            );
            change_audience(&j, member.user_id, thread, change).await;
            gate.release().await;
            drive.await;
        }
        let facts = j.facts().await;
        assert_eq!(facts["ceiling"], 5);
        assert_eq!(facts["content"], 5);
        assert_eq!(facts["state"], "failed");
        assert_eq!(facts["proofs"], 1);
        assert_eq!(facts["reply_levels"], json!([]));
        assert_eq!(facts["delivered"], 0);
        assert_eq!(j.transport.calls.load(Ordering::SeqCst), 0);
        // The current audience is rechecked before another model entry, after the
        // genuine search committed its classified observation. No private tool result
        // may reach a second provider call after CC/demotion changes the audience.
        let requests = j.d.llm.observed();
        assert_eq!(requests.len(), 1);
        assert!(
            requests
                .iter()
                .all(|request| !request.to_string().contains(PRIVATE))
        );
        assert_typed_attention(&j).await;
        j.d.knowledge.shutdown().await.unwrap();
        assert_eq!(j.d.llm.finish().await.unwrap(), 1);
    }
}
