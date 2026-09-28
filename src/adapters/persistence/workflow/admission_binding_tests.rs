use super::super::admission_binding::SourceKey;
use super::*;
use binding_tests::BindingFixture;
use serde_json::json;

fn trigger(company: CompanyId, source: TriggerSource) -> TriggerRef {
    TriggerRef::new(company, TriggerId::new(Uuid::new_v4()), source).unwrap()
}

async fn select(
    f: &BindingFixture,
    key: &str,
    trigger: &TriggerRef,
) -> AppResult<Option<Arc<binding::ConfiguredBinding>>> {
    f.fixture
        .persistence
        .admission_binding(
            f.target.company,
            f.target.binding,
            &IdempotencyKey::parse(key).unwrap(),
            trigger,
        )
        .await
}

// This slice implements the read port only. Seed an already committed snapshot
// directly; these fixtures intentionally claim no admission-write atomicity.
async fn seed_saved(f: &BindingFixture, key: &str, trigger: &TriggerRef) -> Uuid {
    let run = Uuid::new_v4();
    let mut tx = f.fixture.persistence.pool().begin().await.unwrap();
    sqlx::query("INSERT INTO workflow_runs (company_id, id, binding_id, binding_revision, workflow_id, version_id, actor_id, trigger_id, correlation_id, bundle, input, params, resources, max_steps, max_context_bytes, deadline) SELECT company_id, $2, $3, 1, workflow_id, id, actor_id, $4, $5, bundle, '{}'::jsonb, $6, '{}'::jsonb, 100, 1048576, CURRENT_TIMESTAMP + interval '1 hour' FROM workflow_versions WHERE company_id = $1 AND id = $7")
        .bind(f.target.company.as_uuid()).bind(run).bind(f.target.binding.as_uuid())
        .bind(trigger.trigger_id().as_uuid()).bind(Uuid::new_v4()).bind(json!({"saved": 7}))
        .bind(f.request(None).version.as_uuid()).execute(&mut *tx).await.unwrap();
    sqlx::query("INSERT INTO workflow_admissions (company_id, binding_id, source_key, run_id) VALUES ($1, $2, $3, $4)")
        .bind(f.target.company.as_uuid()).bind(f.target.binding.as_uuid())
        .bind(SourceKey::from_trigger(trigger).as_str()).bind(run).execute(&mut *tx).await.unwrap();
    sqlx::query("INSERT INTO workflow_admission_commands (company_id, command_key, run_id, trigger_id) VALUES ($1, $2, $3, $4)")
        .bind(f.target.company.as_uuid()).bind(key).bind(run).bind(trigger.trigger_id().as_uuid())
        .execute(&mut *tx).await.unwrap();
    tx.commit().await.unwrap();
    run
}

#[tokio::test]
async fn workflow_admission_binding_active_and_archived_selection() {
    let f = BindingFixture::new(json!([])).await;
    let event = trigger(f.target.company, TriggerSource::Manual);
    assert!(select(&f, "new", &event).await.unwrap().is_none());
    let inactive = f.service().configure(f.request(None)).await.unwrap();
    assert!(select(&f, "new", &event).await.unwrap().is_none());
    f.service()
        .activate(SetBindingActivity {
            target: f.target,
            expected: inactive.revision,
        })
        .await
        .unwrap();
    let selected = select(&f, "new", &event).await.unwrap().unwrap();
    assert_eq!(selected.id(), f.target.binding);
    assert_eq!(selected.company_id(), f.target.company);
    assert_eq!(selected.revision().get(), 1);
    sqlx::query("UPDATE workflow_definitions SET archived = true WHERE company_id = $1")
        .bind(f.target.company.as_uuid())
        .execute(f.fixture.persistence.pool())
        .await
        .unwrap();
    assert!(select(&f, "new", &event).await.unwrap().is_none());
}

#[tokio::test]
async fn workflow_admission_binding_replay_survives_changed_and_removed_heads() {
    let f = BindingFixture::new(json!([])).await;
    let old = f.service().configure(f.request(None)).await.unwrap();
    let event = trigger(
        f.target.company,
        TriggerSource::Message {
            message_id: crate::domain::entities::message::CanonicalMessageId::new(Uuid::new_v4()),
        },
    );
    seed_saved(&f, "original", &event).await;
    let mut changed = f.request(Some(old.revision));
    changed.params = json!({"changed": 99});
    f.service().configure(changed).await.unwrap();
    let saved = select(&f, "original", &event).await.unwrap().unwrap();
    assert_eq!(saved.revision().get(), 1);
    assert_eq!(saved.params(), &json!({"saved": 7}));
    let redelivery = trigger(f.target.company, event.source().clone());
    let alias = select(&f, "alias", &redelivery).await.unwrap().unwrap();
    assert_eq!(
        store_bundle(saved.bundle()).unwrap(),
        store_bundle(alias.bundle()).unwrap()
    );
    assert_eq!(saved.params(), alias.params());
    let pool = f.fixture.persistence.pool();
    sqlx::query("DELETE FROM workflow_bindings WHERE company_id = $1 AND id = $2")
        .bind(f.target.company.as_uuid())
        .bind(f.target.binding.as_uuid())
        .execute(pool)
        .await
        .unwrap();
    sqlx::query("UPDATE workflow_definitions SET archived = true WHERE company_id = $1")
        .bind(f.target.company.as_uuid())
        .execute(pool)
        .await
        .unwrap();
    assert_eq!(
        select(&f, "original", &event)
            .await
            .unwrap()
            .unwrap()
            .params(),
        saved.params()
    );
    assert_eq!(
        select(&f, "another-alias", &redelivery)
            .await
            .unwrap()
            .unwrap()
            .params(),
        saved.params()
    );
    let other_message = trigger(
        f.target.company,
        TriggerSource::Message {
            message_id: crate::domain::entities::message::CanonicalMessageId::new(Uuid::new_v4()),
        },
    );
    assert!(select(&f, "new", &other_message).await.unwrap().is_none());
    let aliases: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM workflow_admission_commands WHERE company_id = $1",
    )
    .bind(f.target.company.as_uuid())
    .fetch_one(pool)
    .await
    .unwrap();
    assert_eq!(aliases, 1, "read preparation never writes command aliases");
}

#[tokio::test]
async fn workflow_admission_binding_command_conflicts_and_company_scope() {
    let f = BindingFixture::new(json!([])).await;
    let event = trigger(
        f.target.company,
        TriggerSource::Message {
            message_id: crate::domain::entities::message::CanonicalMessageId::new(Uuid::new_v4()),
        },
    );
    seed_saved(&f, "original", &event).await;
    let redelivery = trigger(f.target.company, event.source().clone());
    assert!(matches!(
        select(&f, "original", &redelivery).await,
        Err(AppError::Conflict(_))
    ));
    let changed =
        TriggerRef::new(f.target.company, event.trigger_id(), TriggerSource::Manual).unwrap();
    assert!(matches!(
        select(&f, "original", &changed).await,
        Err(AppError::Conflict(_))
    ));
    assert!(matches!(
        f.fixture
            .persistence
            .admission_binding(
                f.target.company,
                WorkflowBindingId::new(Uuid::new_v4()),
                &IdempotencyKey::parse("original").unwrap(),
                &event
            )
            .await,
        Err(AppError::Conflict(_))
    ));
    let foreign_company = CompanyId::new(Uuid::new_v4());
    let foreign =
        TriggerRef::new(foreign_company, event.trigger_id(), event.source().clone()).unwrap();
    assert!(select(&f, "alias", &foreign).await.is_err());
    assert!(
        f.fixture
            .persistence
            .admission_binding(
                foreign_company,
                f.target.binding,
                &IdempotencyKey::parse("original").unwrap(),
                &foreign
            )
            .await
            .unwrap()
            .is_none()
    );
}

#[tokio::test]
async fn workflow_admission_binding_corrupt_saved_snapshot_fails_without_fallback() {
    let f = BindingFixture::new(json!([])).await;
    let inactive = f.service().configure(f.request(None)).await.unwrap();
    f.service()
        .activate(SetBindingActivity {
            target: f.target,
            expected: inactive.revision,
        })
        .await
        .unwrap();
    let event = trigger(f.target.company, TriggerSource::Manual);
    let run = seed_saved(&f, "original", &event).await;
    let pool = f.fixture.persistence.pool();
    let original = select(&f, "original", &event).await.unwrap().unwrap();
    let bytes = store_bundle(original.bundle()).unwrap();
    let mut unknown: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
    unknown["format"] = json!("unknown");
    for corrupt in [b"invalid".to_vec(), serde_json::to_vec(&unknown).unwrap()] {
        sqlx::query("UPDATE workflow_runs SET bundle = $3 WHERE company_id = $1 AND id = $2")
            .bind(f.target.company.as_uuid())
            .bind(run)
            .bind(corrupt)
            .execute(pool)
            .await
            .unwrap();
        assert!(select(&f, "original", &event).await.is_err());
    }
    sqlx::query("UPDATE workflow_runs SET bundle = $3, resources = '{\"undeclared\": 7}'::jsonb WHERE company_id = $1 AND id = $2")
        .bind(f.target.company.as_uuid()).bind(run).bind(bytes).execute(pool).await.unwrap();
    assert!(select(&f, "original", &event).await.is_err());
    assert!(
        select(
            &f,
            "unrelated",
            &trigger(f.target.company, TriggerSource::Manual)
        )
        .await
        .unwrap()
        .is_some()
    );
}

#[test]
fn workflow_admission_source_key_has_stable_versioned_identity() {
    let company = CompanyId::new(Uuid::new_v4());
    let id = Uuid::new_v4();
    let sources = [
        TriggerSource::Manual,
        TriggerSource::Message {
            message_id: crate::domain::entities::message::CanonicalMessageId::new(id),
        },
        TriggerSource::Schedule {
            schedule_id: ScheduleId::new(id),
            occurrence_id: ScheduleOccurrenceId::new(id),
        },
        TriggerSource::Child {
            parent: ChildCause::Execution(ExecutionRef::new(
                company,
                RunId::new(id),
                ExecutionId::new(id),
                StepId::parse("a".repeat(128)).unwrap(),
            )),
        },
        TriggerSource::Child {
            parent: ChildCause::Action(ActionRef::new(
                ExecutionRef::new(
                    company,
                    RunId::new(id),
                    ExecutionId::new(id),
                    StepId::parse("entry").unwrap(),
                ),
                ActionInvocationId::new(id),
            )),
        },
    ];
    let mut keys = std::collections::BTreeSet::new();
    for source in sources {
        let event = trigger(company, source.clone());
        let first = SourceKey::from_trigger(&event);
        let second = SourceKey::from_trigger(&trigger(company, source.clone()));
        assert!(first.as_str().starts_with("v1:"));
        assert!(first.as_str().len() <= 512);
        assert_eq!(
            first.as_str() == second.as_str(),
            !matches!(source, TriggerSource::Manual)
        );
        assert!(keys.insert(first.as_str().to_owned()));
    }
}
