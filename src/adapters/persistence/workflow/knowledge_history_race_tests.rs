//! Actual PostgreSQL history-read transactions compete with the common intent freezer.
use super::*;
use crate::application::workflow::{
    actions::*,
    history::WorkflowHistory,
    message::{MessageKind, builtin_message_snapshot},
};
use crate::entities::access_level::AccessLevel;

struct RaceSetup {
    fixture: AdmissionFixture,
    conversation: Conversation,
    claim: ClaimedWorkflow,
    recipient: Uuid,
}
async fn setup() -> RaceSetup {
    let f =
        AdmissionFixture::with_binding(BindingFixture::from_source(source(131_072, 65_536)).await)
            .await;
    let c = Conversation::new(&f).await;
    let actor =
        super::super::super::message_tests::support::member(&f.binding.fixture, "admin").await;
    let recipient =
        super::super::super::message_tests::support::member(&f.binding.fixture, "member").await;
    let company = f.binding.target.company.as_uuid();
    sqlx::query("UPDATE company_members SET access_level=5 WHERE company_id=$1 AND user_id=$2")
        .bind(company)
        .bind(actor)
        .execute(f.persistence().pool())
        .await
        .unwrap();
    sqlx::query("INSERT INTO thread_principals(company_id,channel_id,thread_id,principal_id,role) SELECT $1,$2,$3,id,'participant' FROM principals WHERE company_id=$1 AND user_id=$4")
        .bind(company).bind(c.channel).bind(c.thread).bind(actor).execute(f.persistence().pool()).await.unwrap();
    super::super::knowledge_admission_tests::stamped(&f, &c, AccessLevel::L5).await;
    let claim = claimed(&f, &c, "freeze-read-race").await;
    RaceSetup {
        fixture: f,
        conversation: c,
        claim,
        recipient,
    }
}
fn request(
    f: &AdmissionFixture,
    claim: &ClaimedWorkflow,
    kind: &str,
    destination: Uuid,
    body: &str,
) -> ActionRequest {
    let scope = claim.fence.scope;
    let contract = builtin_message_snapshot(scope.company, MessageKind::Send).unwrap();
    ActionRequest {
        scope: ActionScope {
            company: scope.company,
            run: scope.run,
            execution: scope.execution,
        },
        operation_key: contract.contract.name.clone(),
        target: ActionTarget::Local {
            resource: scope.company.as_uuid(),
            resource_kind: TypeName::parse("messaging").unwrap(),
        },
        arguments: json!({"content":{"subject":"Racing branch","body":body},"destinations":[{"kind":kind,"id":destination.to_string()}]}),
        context: ActionPolicyContext {
            actor: f.binding.target.actor.user_id(),
            capability_ceiling: vec![contract.policy.capability.clone()],
            approval_required: false,
        },
        contract,
    }
}
async fn freeze_facts(f: &AdmissionFixture, intent: ActionIntent) -> Value {
    sqlx::query_scalar("SELECT jsonb_build_object('content',content_level_at_freeze,'destination',destination_level_at_freeze) FROM workflow_action_intents WHERE id=$1")
        .bind(intent.invocation.as_uuid()).fetch_one(f.persistence().pool()).await.unwrap()
}
#[tokio::test]
async fn knowledge_run_level_race_history_read_and_email_freeze_record_consistent_consumption() {
    // Explicitly cover both orders, then repeatedly run real competing transactions.
    for order in 0..14 {
        let RaceSetup {
            fixture: f,
            conversation: c,
            claim,
            recipient,
        } = setup().await;
        let p = f.persistence();
        let service = ActionService::new(p.clone());
        let public = request(&f, &claim, "user", recipient, "Independent public branch");
        let (read, freeze) = match order {
            0 => {
                let freeze = service.prepare_step(public).await;
                let read = p.frozen_history(&claim).await;
                (read, freeze)
            }
            1 => {
                let read = p.frozen_history(&claim).await;
                let freeze = service.prepare_step(public).await;
                (read, freeze)
            }
            _ => {
                let barrier = tokio::sync::Barrier::new(2);
                tokio::join!(
                    async {
                        barrier.wait().await;
                        p.frozen_history(&claim).await
                    },
                    async {
                        barrier.wait().await;
                        service.prepare_step(public).await
                    }
                )
            }
        };
        let read = read.unwrap();
        assert_eq!(read.newest.len(), 2);
        match freeze {
            Ok(intent) => {
                assert_ne!(order, 1);
                assert_eq!(
                    freeze_facts(&f, intent).await,
                    json!({"content":0,"destination":0})
                );
            }
            Err(AppError::KnowledgeRelease {
                content: AccessLevel::L5,
                destination: AccessLevel::L0,
            }) => assert_ne!(order, 0),
            other => panic!("unexpected race result {other:?}"),
        }
        // A branch consuming the returned classified output must freeze at least that level.
        let body = read
            .newest
            .iter()
            .map(|m| m.body.as_str())
            .collect::<Vec<_>>()
            .join("\n");
        assert!(matches!(
            service
                .prepare_step(request(&f, &claim, "user", recipient, &body))
                .await,
            Err(AppError::KnowledgeRelease { .. })
        ));
        let safe = service
            .prepare_step(request(&f, &claim, "channel", c.channel, &body))
            .await
            .unwrap();
        assert_eq!(
            freeze_facts(&f, safe).await,
            json!({"content":5,"destination":5})
        );
        let effects: i64 =
            sqlx::query_scalar("SELECT count(*) FROM workflow_action_dispatches WHERE run_id=$1")
                .bind(claim.fence.scope.run.as_uuid())
                .fetch_one(p.pool())
                .await
                .unwrap();
        assert_eq!(effects, 0);
    }
}
