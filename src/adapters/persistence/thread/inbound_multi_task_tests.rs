use super::*;
use crate::transport::MessageDisposition;

#[derive(Debug, PartialEq, Eq, sqlx::FromRow)]
struct StoredIntent {
    channel_id: Uuid,
    thread_id: Uuid,
    role: String,
}

async fn audience(fixture: &Fixture, message: CanonicalMessageId) -> String {
    sqlx::query_scalar("SELECT audience FROM messages WHERE company_id = $1 AND id = $2")
        .bind(fixture.company_id)
        .bind(message.as_uuid())
        .fetch_one(&fixture.pool)
        .await
        .unwrap()
}

pub(super) async fn two_channel_request(fixture: &Fixture, rfc: &str) -> InboundCommitRequest {
    let second_channel = fixture.extra_channel("billing").await;
    let second_binding = fixture.email_binding_of(second_channel).await;
    let mut request = request(fixture, rfc, "Please answer from both teams").await;
    let mut associations = request.associations.into_inner();
    associations.push(ThreadAssociation {
        channel_id: second_channel,
        binding_id: second_binding,
        target: ThreadTarget::Create {
            subject: "Quick question".into(),
        },
        role: RecipientRole::Cc,
        step: PipelineStep::only(),
        principals: sender_principals(),
    });
    request.associations = BoundedVec::parse("thread associations", associations).unwrap();
    let mut intents = request.intents.into_inner();
    intents.push(intent(second_channel, RecipientRole::Cc));
    request.intents = BoundedVec::parse("admission intents", intents).unwrap();
    request
}

/// Each association gets its own intent with its own role and thread; a redelivery returns them
/// and writes none (p08 D1.3).
#[tokio::test]
async fn each_association_gets_one_intent_and_redelivery_adds_none() {
    let Some(fixture) = Fixture::new("inbound_two_intents").await else {
        return;
    };
    let rfc = format!("<two-intents-{}@example.com>", fixture.suffix);
    let request = two_channel_request(&fixture, &rfc).await;
    let first = fixture
        .persistence
        .commit_inbound(request.clone())
        .await
        .unwrap();
    assert_eq!(first.intent_ids.len(), 2);
    for (index, intent_id) in first.intent_ids.iter().enumerate() {
        let stored: StoredIntent = sqlx::query_as(
            "SELECT channel_id, thread_id, role FROM workflow_message_intents \
             WHERE company_id = $1 AND id = $2",
        )
        .bind(fixture.company_id)
        .bind(intent_id)
        .fetch_one(&fixture.pool)
        .await
        .unwrap();
        assert_eq!(
            stored,
            StoredIntent {
                channel_id: request.intents[index].channel_id,
                thread_id: first.thread_ids[index],
                role: if index == 0 { "to" } else { "cc" }.to_owned(),
            }
        );
    }
    // An answering message is external conversation, on the copied association too, whether or
    // not any binding admits it (p08 D1.3).
    assert_eq!(
        audience(&fixture, first.message_id).await,
        "external_conversation"
    );
    let second = fixture.persistence.commit_inbound(request).await.unwrap();
    assert_eq!(second.disposition, CommitDisposition::Duplicate);
    assert_eq!(second.message_id, first.message_id);
    assert_eq!(second.thread_ids, first.thread_ids);
    assert_eq!(second.intent_ids, first.intent_ids);
    assert_eq!(intent_count(&fixture).await, 2);
    assert_eq!(message_count(&fixture).await, 1);
    fixture.cleanup().await;
}

/// A quiet message is filed for the team with an inadmissible intent, a simulation's in-app policy
/// reaches the intent as stated, and a relayed message's hop count and trace are recorded for the
/// workflow reply to continue (p08 D1.3, D1.6, D1.7).
#[tokio::test]
async fn an_intent_records_disposition_policy_origin_and_relay_trace() {
    let Some(fixture) = Fixture::new("inbound_intent_facts").await else {
        return;
    };
    let rfc = format!("<facts-{}@example.com>", fixture.suffix);
    let mut request = request(&fixture, &rfc, "Filed only").await;
    let traced = Uuid::new_v4();
    request.envelope.directives.hop_count = 2;
    request.envelope.directives.trace_channels =
        BoundedVec::parse("trace channels", vec![traced]).unwrap();
    request.envelope.directives.disposition = MessageDisposition::FileOnly;
    request.admissible = false;
    request.origin = InboundOrigin::Application;
    request.reply_delivery = ReplyDelivery::InAppOnly;
    let outcome = fixture.persistence.commit_inbound(request).await.unwrap();
    let stored: (bool, String, String, i32, Vec<Uuid>) = sqlx::query_as(
        "SELECT admissible, origin, delivery_policy, hop_count, trace_channels \
         FROM workflow_message_intents WHERE company_id = $1 AND id = $2",
    )
    .bind(fixture.company_id)
    .bind(outcome.intent_ids[0])
    .fetch_one(&fixture.pool)
    .await
    .unwrap();
    assert_eq!(
        stored,
        (
            false,
            "application".to_string(),
            "in_app_only".to_string(),
            2,
            vec![traced]
        )
    );
    assert_eq!(
        audience(&fixture, outcome.message_id).await,
        "internal_only",
        "a quiet message is filed for the team"
    );
    fixture.cleanup().await;
}

/// An intent names its own company's message association (p08 F008.4). Its binding, run,
/// answered-message and responder references are covered by the workflow test
/// `message_intent_rows_cannot_name_another_companys_records`.
#[tokio::test]
async fn intent_rows_cannot_reach_across_tenants() {
    let Some(fixture) = Fixture::new("inbound_intent_tenancy").await else {
        return;
    };
    let rfc = format!("<tenancy-{}@example.com>", fixture.suffix);
    let outcome = fixture
        .persistence
        .commit_inbound(request(&fixture, &rfc, "Hello").await)
        .await
        .unwrap();
    let foreign = fixture.foreign_company().await;
    // The association of this company's message, claimed by another company.
    let cross = sqlx::query(
        "INSERT INTO workflow_message_intents (company_id, id, message_id, channel_id, thread_id, \
           role, mentioned, admissible, origin, hop_count, trace_channels, delivery_policy, \
           correlation_id) \
         VALUES ($1, $2, $3, $4, $5, 'to', false, true, 'external', 0, '{}', 'deliver', $2)",
    )
    .bind(foreign)
    .bind(Uuid::new_v4())
    .bind(outcome.message_id.as_uuid())
    .bind(fixture.channel_id)
    .bind(outcome.thread_ids[0])
    .execute(&fixture.pool)
    .await;
    assert!(
        cross.is_err(),
        "an intent must name its own company's association"
    );
    fixture.cleanup().await;
}
