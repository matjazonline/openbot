use super::*;
use crate::application::workflow::decisions::{
    AddDecisionComment, DecisionDisposition, DecisionSubmission,
};

struct HistoricalReply {
    fixture: FinalFixture,
    retired: Uuid,
    command: SubmitDecision,
    comment: AddDecisionComment,
    submitted: DecisionSubmission,
}

async fn historical_reply() -> HistoricalReply {
    let s = Box::pin(Intents::new()).await;
    let retired = member(&s.fixture, "admin").await;
    let reviewer = s.fixture.principal(retired).await;
    let mut source: Value =
        serde_json::from_str(&registry::example("decision.human").unwrap().source).unwrap();
    source["steps"]["start"]["with"]["reviewers"] =
        json!({"literal":[{"kind":"principal","id":s.owner_principal()}]});
    source["steps"]["start"]["with"]["data_schema"] = json!({"literal":true});
    source["steps"]["start"]["with"]["feedback_required"] = json!({"literal":[]});
    source["steps"]["start"]["with"]["deadline"] =
        json!({"literal":(chrono::Utc::now()+chrono::Duration::minutes(20)).to_rfc3339()});
    let fixture = Box::pin(FinalFixture::from_case(
        s,
        FinalCase {
            source,
            contract: literal_contract("Complete"),
            policy: Some(ReplyApprovalPolicy::new(vec![reviewer], 60).unwrap()),
            run_as: None,
            publication: FinalPublication::Standard,
            delivery_policy: DeliveryPolicy::Deliver,
        },
    ))
    .await;
    assert!(
        fixture
            .p
            .park_decision(fixture.scope, &approval_test_support::notices(&fixture.p))
            .await
            .unwrap()
            .is_some()
    );
    let decision=DecisionId(sqlx::query_scalar("SELECT id FROM workflow_decisions WHERE company_id=$1 AND run_id=$2 AND subject_kind='business'")
        .bind(fixture.scope.company.as_uuid()).bind(fixture.scope.run.as_uuid()).fetch_one(fixture.p.pool()).await.unwrap());
    let view = fixture
        .p
        .read_decision(fixture.scope.company, fixture.s.owner(), decision)
        .await
        .unwrap();
    let comment = AddDecisionComment {
        company: fixture.scope.company,
        actor: fixture.s.owner(),
        decision,
        command_key: DecisionCommandKey::parse("before-policy-retirement").unwrap(),
        body: "Recorded before settlement".into(),
    };
    fixture
        .p
        .add_decision_comment(comment.clone())
        .await
        .unwrap();
    let command = SubmitDecision {
        company: fixture.scope.company,
        actor: fixture.s.owner(),
        decision,
        command_key: DecisionCommandKey::parse("recorded-business-vote").unwrap(),
        expected_revision: view.revision,
        expected_artifact: view.artifact_digest,
        choice: view.choices[0].clone(),
        data: json!(null),
        feedback: String::new(),
    };
    let submitted = fixture.p.submit_decision(command.clone()).await.unwrap();
    HistoricalReply {
        fixture,
        retired,
        command,
        comment,
        submitted,
    }
}

#[tokio::test]
async fn workflow_reviewer_follow_history_and_replays_do_not_require_unused_retired_reply_policy_accounts()
 {
    let HistoricalReply {
        fixture: f,
        retired,
        command,
        comment,
        submitted,
    } = Box::pin(historical_reply()).await;
    let before =
        f.p.read_decision(f.scope.company, f.s.owner(), command.decision)
            .await
            .unwrap();
    f.p.delete_user_account(retired).await.unwrap();
    let mut check = f.p.pool().begin().await.unwrap();
    let missing = crate::adapters::persistence::workflow::action_authority::prelock_accounts_on(
        &mut check,
        crate::application::workflow::actions::ActionScope {
            company: f.scope.company,
            run: f.scope.run,
            execution: f.scope.execution,
        },
        Some(f.s.owner()),
    )
    .await;
    assert!(
        matches!(missing, Err(AppError::NotFound(_))),
        "the retired policy mapping really is unusable for a fresh effect: {missing:?}"
    );
    check.rollback().await.unwrap();
    let after =
        f.p.read_decision(f.scope.company, f.s.owner(), command.decision)
            .await
            .unwrap();
    assert_eq!(
        before, after,
        "historical view must not gain a retired-policy prerequisite"
    );
    let replay = f.p.submit_decision(command.clone()).await.unwrap();
    assert_eq!(replay.disposition, DecisionDisposition::Replayed);
    assert_eq!(replay.revision, submitted.revision);
    assert_eq!(replay.successor, submitted.successor);
    let replayed = f.p.add_decision_comment(comment.clone()).await.unwrap();
    assert_eq!(replayed.disposition, DecisionDisposition::Replayed);
    let changed = AddDecisionComment {
        body: "Changed replay".into(),
        ..comment.clone()
    };
    assert!(matches!(
        f.p.add_decision_comment(changed).await,
        Err(AppError::Conflict(_))
    ));
    let fresh = AddDecisionComment {
        command_key: DecisionCommandKey::parse("new-closed-comment").unwrap(),
        ..comment
    };
    assert!(
        matches!(
            f.p.add_decision_comment(fresh).await,
            Err(AppError::Conflict(_))
        ),
        "a fresh closed comment retains its existing policy"
    );
    assert_eq!(
        f.p.read_decision(f.scope.company, f.s.owner(), command.decision)
            .await
            .unwrap(),
        after
    );
    let old_actor = WorkflowActor::authenticated(retired).unwrap();
    assert!(
        matches!(
            f.p.read_decision(f.scope.company, old_actor, command.decision)
                .await,
            Err(AppError::NotFound(_))
        ),
        "retired caller authority is still denied"
    );
}
