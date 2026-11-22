//! `unit_archive_runs_` through the production ingest and engine (Plan08 02B.2–3): a message an
//! active desk accepted is refused at admission once its unit is archived, recording
//! `unit.archived` and no run, and a message sent to the archived desk is bounced before any
//! intent exists.
use super::*;
use crate::application::ports::department::OpenWork;
use crate::entities::department::DepartmentId;

#[tokio::test]
async fn unit_archive_runs_message_to_an_archived_units_desk_admits_nothing() {
    let d = Deployment::new(vec![]).await;
    let binding = d
        .bind(answering_source(), Some(MessageTrigger::Direct))
        .await;
    let company = d.fixture.target.company.as_uuid();
    let owner = d.fixture.target.actor.user_id();
    let unit = d.p().test_unit(company, "front-office").await;
    sqlx::query("UPDATE channels SET department_id=$3 WHERE company_id=$1 AND id=$2")
        .bind(company)
        .bind(d.channel)
        .bind(unit)
        .execute(d.p().pool())
        .await
        .unwrap();

    // Accepted while the unit was active; the intent waits for a worker.
    let accepted = d.email(customer_email(&d, "before", None)).await;
    assert_eq!(accepted.intent_ids.len(), 1);
    let departments = crate::adapters::persistence::test_support::department_use_cases(d.p());
    departments
        .archive_department(owner, company, DepartmentId::new(unit), OpenWork::Refuse)
        .await
        .expect("a pending message intent is not open work");

    let intent = accepted.intent_ids[0];
    d.run_until("the intent settles", async || {
        d.count(&format!(
            "SELECT count(*) FROM workflow_message_intents WHERE company_id = $1 \
             AND id = '{intent}' AND state = 'done'"
        ))
        .await
            == 1
    })
    .await;
    let outcome: (String, Option<Uuid>, Option<String>) = sqlx::query_as(
        "SELECT outcome, run_id, reason FROM workflow_message_intent_bindings \
         WHERE company_id = $1 AND intent_id = $2 AND binding_id = $3",
    )
    .bind(company)
    .bind(intent)
    .bind(binding.binding.as_uuid())
    .fetch_one(d.p().pool())
    .await
    .unwrap();
    assert_eq!(
        outcome,
        ("refused".into(), None, Some("unit.archived".into()))
    );
    assert_eq!(
        d.count("SELECT count(*) FROM workflow_runs WHERE company_id = $1")
            .await,
        0,
        "no run on an archived unit's desk"
    );

    // Sent after the archive: bounced at ingest, so no intent is recorded.
    let intents = d
        .count("SELECT count(*) FROM workflow_message_intents WHERE company_id = $1")
        .await;
    let bounced = d
        .threads
        .ingest_test_email(customer_email(&d, "after", None))
        .await
        .unwrap();
    assert!(bounced.intent_ids.is_empty(), "{:?}", bounced.reason());
    assert_eq!(
        d.count("SELECT count(*) FROM workflow_message_intents WHERE company_id = $1")
            .await,
        intents
    );
    assert_eq!(
        d.llm.finish().await.unwrap(),
        0,
        "the model is never called"
    );
}
