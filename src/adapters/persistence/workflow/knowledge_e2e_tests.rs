//! Sales-document journeys through actual admission, workflow, message and delivery workers.
use super::*;
use crate::entities::access_level::AccessLevel;
use std::sync::atomic::Ordering;

#[path = "knowledge_e2e_support.rs"]
mod support;
use support::*;

#[path = "knowledge_e2e_race_tests.rs"]
mod races;

#[path = "knowledge_e2e_upload_tests.rs"]
mod uploads;

#[path = "knowledge_e2e_mcp_tests.rs"]
mod mcp;

#[tokio::test]
async fn knowledge_e2e_outside_contact_cannot_observe_private_sales_and_gets_missing_information_reply()
 {
    let mut j = Box::pin(SalesJourney::new(
        turns("I don't have that information."),
        Audience::Public,
    ))
    .await;
    j.d.bind(answering_source(), Some(MessageTrigger::Direct))
        .await;
    let ingested = j.d.email(email(&j.d, CUSTOMER, "outside-sales")).await;
    assert_eq!(ingested.intent_ids.len(), 1);
    j.drive(1).await;
    assert_eq!(j.transport.calls.load(Ordering::SeqCst), 1);
    assert!(!observed_private(&mut j));
    let facts = j.facts().await;
    assert_eq!(facts["ceiling"], 0);
    assert_eq!(facts["content"], 0);
    assert_eq!(facts["proofs"], 1);
    assert_eq!(facts["reply_levels"], json!([0]));
    assert_eq!(facts["delivered"], 1);
    let body:String=sqlx::query_scalar("SELECT message.clean_text_body FROM workflow_message_destinations AS destination JOIN messages AS message ON message.company_id=destination.company_id AND message.id=destination.message_id WHERE destination.company_id=$1")
        .bind(j.d.fixture.target.company.as_uuid()).fetch_one(j.d.p().pool()).await.unwrap();
    assert!(body.contains("I don't have that information"));
    assert!(!body.contains(PRIVATE));
    j.d.knowledge.shutdown().await.unwrap();
    assert_eq!(j.d.llm.finish().await.unwrap(), 2);
}

#[tokio::test]
async fn knowledge_e2e_level_five_team_reply_is_stamped_and_level_zero_viewer_gets_placeholder() {
    let mut j = Box::pin(SalesJourney::new(
        turns("sales.md: PRIVATE FLOOR 60"),
        Audience::Team,
    ))
    .await;
    let member = j.member(AccessLevel::L5).await;
    let lower = j.member(AccessLevel::PUBLIC).await;
    j.d.bind(answering_source(), Some(MessageTrigger::Direct))
        .await;
    let ingested =
        j.d.email(email(&j.d, member.email.as_str(), "team-sales"))
            .await;
    let thread = ingested.thread.unwrap().id;
    j.drive(1).await;
    assert!(observed_private(&mut j));
    assert_eq!(j.transport.calls.load(Ordering::SeqCst), 1);
    let facts = j.facts().await;
    assert_eq!(facts["ceiling"], 5);
    assert_eq!(facts["content"], 5);
    assert_eq!(facts["reply_levels"], json!([5]));
    let scope = crate::use_cases::thread::ThreadReadScope {
        channel: crate::use_cases::thread::ChannelReadScope {
            company_id: j.d.fixture.target.company.as_uuid(),
            channel_id: j.d.channel,
        },
        thread_id: thread,
    };
    let messages = j.d.threads.get_thread_history(&lower, scope).await.unwrap();
    let restricted: Vec<_> = messages
        .iter()
        .filter(|message| message.restricted)
        .collect();
    assert_eq!(restricted.len(), 1);
    assert!(restricted[0].attachments.is_empty());
    for message in &messages {
        let html = crate::adapters::http::pages::message_bubble_chat(
            message,
            Some(&lower.email),
            crate::adapters::http::pages::MessageScope {
                company_id: j.d.fixture.target.company.as_uuid(),
                channel_id: j.d.channel,
            },
            crate::adapters::http::pages::RunLinkAccess::Label,
        );
        assert!(!html.contains(PRIVATE));
        if message.restricted {
            assert!(html.contains("Restricted message"));
        }
    }
    j.d.knowledge.shutdown().await.unwrap();
    assert_eq!(j.d.llm.finish().await.unwrap(), 2);
}

#[tokio::test]
async fn knowledge_e2e_customer_cc_lowers_member_run_to_public_before_search() {
    let mut j = Box::pin(SalesJourney::new(
        turns("I don't have that information."),
        Audience::Public,
    ))
    .await;
    let member = j.member(AccessLevel::L5).await;
    j.d.bind(answering_source(), Some(MessageTrigger::Direct))
        .await;
    let mut incoming = email(&j.d, member.email.as_str(), "cc-sales");
    incoming.cc = Some(CUSTOMER.into());
    let ingested = j.d.email(incoming).await;
    assert_eq!(ingested.intent_ids.len(), 1);
    j.drive(1).await;
    assert!(!observed_private(&mut j));
    let facts = j.facts().await;
    assert_eq!(facts["ceiling"], 0);
    assert_eq!(facts["content"], 0);
    assert_eq!(facts["reply_levels"], json!([0]));
    assert_eq!(j.transport.calls.load(Ordering::SeqCst), 1);
    j.d.knowledge.shutdown().await.unwrap();
    assert_eq!(j.d.llm.finish().await.unwrap(), 2);
}

#[tokio::test]
async fn knowledge_e2e_member_upload_ceiling_prefix_new_layer_and_replacement_are_enforced() {
    let j = Box::pin(SalesJourney::new(Vec::new(), Audience::Team)).await;
    let member = j.member(AccessLevel::L3).await;
    let actor = scope(&j.d, member.user_id);
    assert!(
        j.d.knowledge
            .upload(actor, vec![file("5_sales.md", "forged replacement")])
            .await
            .is_err()
    );
    j.d.knowledge
        .upload(actor, vec![file("3_sales.md", "team price 80")])
        .await
        .unwrap();
    j.d.knowledge
        .upload(actor, vec![file("3_sales.md", "team price 75")])
        .await
        .unwrap();
    assert!(
        crate::application::knowledge::KnowledgeUploadFile::from_upload(
            "sales.md",
            b"no prefix".to_vec()
        )
        .is_err()
    );
    let layers:Vec<(i16,i64,String)>=sqlx::query_as("SELECT access_level::smallint,revision,content FROM knowledge_documents WHERE company_id=$1 ORDER BY access_level")
        .bind(j.d.fixture.target.company.as_uuid()).fetch_all(j.d.p().pool()).await.unwrap();
    assert_eq!(
        layers.iter().map(|row| row.0).collect::<Vec<_>>(),
        vec![0, 3, 5]
    );
    assert_eq!((layers[1].1, layers[1].2.as_str()), (2, "team price 75"));
    assert_eq!(layers[2].2, "AX-4410 PRIVATE FLOOR 60");
    j.d.knowledge.shutdown().await.unwrap();
    assert_eq!(j.d.llm.finish().await.unwrap(), 0);
}

#[tokio::test]
async fn knowledge_e2e_public_followup_in_the_same_thread_excludes_prior_private_reply_from_history()
 {
    let mut scripted = turns("sales.md: PRIVATE FLOOR 60");
    scripted.extend(turns("I don't have that information for the customer."));
    let mut j = Box::pin(SalesJourney::new(scripted, Audience::Team)).await;
    let member = j.member(AccessLevel::L5).await;
    j.d.bind(answering_source(), Some(MessageTrigger::Direct))
        .await;
    let first =
        j.d.email(email(&j.d, member.email.as_str(), "private-before-summary"))
            .await;
    j.drive(1).await;
    let first_requests = j.d.llm.observed();
    assert_eq!(first_requests.len(), 2);
    assert!(
        first_requests
            .iter()
            .any(|request| request.to_string().contains(PRIVATE))
    );
    let mut followup = email(&j.d, member.email.as_str(), "public-summary");
    followup.text = Some("Summarise this thread for the customer.".into());
    followup.cc = Some(CUSTOMER.into());
    followup.headers = Some("Message-ID: <public-summary@partner.test>\nIn-Reply-To: <private-before-summary@partner.test>\n".into());
    let second = j.d.email(followup).await;
    assert_eq!(second.thread.unwrap().id, first.thread.unwrap().id);
    j.drive(2).await;
    let requests = j.d.llm.observed();
    assert_eq!(requests.len(), 2);
    assert!(
        requests
            .iter()
            .all(|request| !request.to_string().contains(PRIVATE))
    );
    let facts = j.facts().await;
    assert_eq!(facts["ceiling"], 0);
    assert_eq!(facts["content"], 0);
    assert_eq!(facts["reply_levels"], json!([0]));
    assert_eq!(j.transport.calls.load(Ordering::SeqCst), 2);
    j.d.knowledge.shutdown().await.unwrap();
    assert_eq!(j.d.llm.finish().await.unwrap(), 4);
}
