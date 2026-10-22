use super::*;

fn with_source(
    company_id: CompanyId,
    version_id: VersionId,
    key: &str,
    source: TriggerSource,
    correlation_id: CorrelationId,
) -> AdmitWorkflowRequest {
    let mut command = request(company_id, version_id, key, json!({"in": key}));
    command.trigger = TriggerRef::new(company_id, trigger_id_for_key(key), source).unwrap();
    command.correlation_id = correlation_id;
    command
}

fn seed_message(store: &MemoryStore, company_id: CompanyId, id: CanonicalMessageId) {
    store
        .state
        .lock()
        .unwrap()
        .source_messages
        .insert((company_id, id), RelatedAssociation::Company);
}

#[tokio::test]
async fn four_sources_share_run_head_and_claim_provenance() {
    let company_id = company();
    let version_id = version();
    let store = MemoryStore::new(owned(company_id, version_id, 256));
    let svc = service(&store);
    let trace = CorrelationId::new();
    let message_id = CanonicalMessageId::random();
    seed_message(&store, company_id, message_id);
    let schedule_id = ScheduleId::new(Uuid::new_v4());
    let occurrence_id = ScheduleOccurrenceId::new(Uuid::new_v4());
    store.state.lock().unwrap().schedule_occurrences.insert(
        (company_id, schedule_id, occurrence_id),
        RelatedAssociation::Company,
    );
    let sources = [
        TriggerSource::Manual,
        TriggerSource::Message { message_id },
        TriggerSource::Schedule {
            schedule_id,
            occurrence_id,
        },
    ];
    let mut parent = None;
    for (index, source) in sources.into_iter().enumerate() {
        let key = format!("source-{index}");
        let command = with_source(company_id, version_id, &key, source.clone(), trace);
        let AdmissionResult::Created(run_id) = svc.admit(command).await.unwrap() else {
            panic!("created")
        };
        let head = store.head(company_id, run_id).await.unwrap().unwrap();
        assert_eq!(head.causality.trigger().source(), &source);
        assert_eq!(head.causality.correlation_id(), trace);
        assert_eq!(head.run_id(), run_id);
        if index == 0 {
            parent = Some(run_id);
        }
    }
    assert_child_source_and_claims(&store, company_id, version_id, trace, parent.unwrap()).await;
}

async fn assert_child_source_and_claims(
    store: &MemoryStore,
    company_id: CompanyId,
    version_id: VersionId,
    trace: CorrelationId,
    parent_run: RunId,
) {
    let svc = service(store);
    let parent_step = {
        let state = store.state.lock().unwrap();
        state
            .jobs
            .iter()
            .find(|job| job.run_id() == parent_run)
            .unwrap()
            .step
            .execution()
            .clone()
    };
    let call = ChildToolCallRef::new(parent_step, ToolCallId::new(Uuid::new_v4()));
    // Category C (07-D1 §D1.2a): no admission creates a child, even for a real parent call.
    let child_source = TriggerSource::Child {
        parent: ChildCause::ToolCall(call),
    };
    let runs = store.state.lock().unwrap().runs.len();
    assert!(matches!(
        svc.admit(with_source(
            company_id,
            version_id,
            "child",
            child_source,
            trace
        ))
        .await,
        Err(AppError::BadRequest(_))
    ));
    assert_eq!(store.state.lock().unwrap().runs.len(), runs);
    let claims = store
        .claim_ready(ClaimRequest {
            worker_id: WorkerId::new(Uuid::new_v4()),
            batch: ClaimBatch::new(4).unwrap(),
            lease: LeaseDuration::new(std::time::Duration::from_secs(30)).unwrap(),
        })
        .await
        .unwrap();
    assert_eq!(claims.len(), 3);
    for claim in claims {
        assert_eq!(
            claim.causality(),
            &store
                .head(company_id, claim.run_id())
                .await
                .unwrap()
                .unwrap()
                .causality
        );
        assert_eq!(claim.step().execution().run_id(), claim.run_id());
        assert_eq!(claim.step().execution().company_id(), company_id);
    }
}

#[tokio::test]
async fn replay_keeps_original_trace_and_source_is_part_of_equivalence() {
    let company_id = company();
    let version_id = version();
    let store = MemoryStore::new(owned(company_id, version_id, 256));
    let svc = service(&store);
    let message_id = CanonicalMessageId::random();
    seed_message(&store, company_id, message_id);
    let original = CorrelationId::new();
    let command = with_source(
        company_id,
        version_id,
        "stable",
        TriggerSource::Message { message_id },
        original,
    );
    let AdmissionResult::Created(run_id) = svc.admit(command).await.unwrap() else {
        panic!("created")
    };
    let changed_trace = CorrelationId::new();
    assert_eq!(
        svc.admit(with_source(
            company_id,
            version_id,
            "stable",
            TriggerSource::Message { message_id },
            changed_trace
        ))
        .await
        .unwrap(),
        AdmissionResult::Replayed(run_id)
    );
    assert_eq!(
        store
            .head(company_id, run_id)
            .await
            .unwrap()
            .unwrap()
            .causality
            .correlation_id(),
        original
    );
    assert_eq!(
        svc.admit(with_source(
            company_id,
            version_id,
            "stable",
            TriggerSource::Manual,
            original
        ))
        .await
        .unwrap(),
        AdmissionResult::Conflict
    );
    assert_changed_trigger_and_input_conflict(
        &store, company_id, version_id, message_id, original, run_id,
    )
    .await;
}

async fn assert_changed_trigger_and_input_conflict(
    store: &MemoryStore,
    company_id: CompanyId,
    version_id: VersionId,
    message_id: CanonicalMessageId,
    original: CorrelationId,
    _run_id: RunId,
) {
    let svc = service(store);
    let mut changed_trigger = with_source(
        company_id,
        version_id,
        "stable",
        TriggerSource::Message { message_id },
        original,
    );
    changed_trigger.trigger = TriggerRef::new(
        company_id,
        TriggerId::new(Uuid::new_v4()),
        TriggerSource::Message { message_id },
    )
    .unwrap();
    assert_eq!(
        svc.admit(changed_trigger).await.unwrap(),
        AdmissionResult::Conflict
    );
    let distinct = svc
        .admit(with_source(
            company_id,
            version_id,
            "other",
            TriggerSource::Message { message_id },
            original,
        ))
        .await
        .unwrap();
    assert_eq!(distinct, AdmissionResult::Conflict);
    assert_eq!(store.state.lock().unwrap().runs.len(), 1);
}

#[tokio::test]
async fn missing_foreign_or_inconsistent_sources_fail_before_any_write() {
    let company_id = company();
    let other = company();
    let version_id = version();
    let store = MemoryStore::new(owned(company_id, version_id, 256));
    let svc = service(&store);
    let trace = CorrelationId::new();
    let message_id = CanonicalMessageId::random();
    seed_message(&store, other, message_id);
    assert!(matches!(
        svc.admit(with_source(
            company_id,
            version_id,
            "foreign-message",
            TriggerSource::Message { message_id },
            trace
        ))
        .await,
        Err(AppError::NotFound(_))
    ));
    store.state.lock().unwrap().source_messages.insert(
        (company_id, message_id),
        RelatedAssociation::Channel(RelatedChannelId::new(Uuid::new_v4())),
    );
    assert!(matches!(
        svc.admit(with_source(
            company_id,
            version_id,
            "wrong-message-association",
            TriggerSource::Message { message_id },
            trace
        ))
        .await,
        Err(AppError::NotFound(_))
    ));
    let schedule_id = ScheduleId::new(Uuid::new_v4());
    let occurrence_id = ScheduleOccurrenceId::new(Uuid::new_v4());
    assert!(matches!(
        svc.admit(with_source(
            company_id,
            version_id,
            "missing-slot",
            TriggerSource::Schedule {
                schedule_id,
                occurrence_id
            },
            trace
        ))
        .await,
        Err(AppError::NotFound(_))
    ));
    store.state.lock().unwrap().schedule_occurrences.insert(
        (company_id, schedule_id, occurrence_id),
        RelatedAssociation::Channel(RelatedChannelId::new(Uuid::new_v4())),
    );
    assert!(matches!(
        svc.admit(with_source(
            company_id,
            version_id,
            "wrong-association",
            TriggerSource::Schedule {
                schedule_id,
                occurrence_id
            },
            trace
        ))
        .await,
        Err(AppError::NotFound(_))
    ));
    assert_missing_parent_and_source_lookup_error(
        &store, company_id, version_id, message_id, trace,
    )
    .await;
}

async fn assert_missing_parent_and_source_lookup_error(
    store: &MemoryStore,
    company_id: CompanyId,
    version_id: VersionId,
    message_id: CanonicalMessageId,
    trace: CorrelationId,
) {
    let svc = service(store);
    let parent = ExecutionRef::new(
        company_id,
        RunId::new(Uuid::new_v4()),
        ExecutionId::new(Uuid::new_v4()),
        step_id("parent"),
    );
    let call = ChildToolCallRef::new(parent.clone(), ToolCallId::new(Uuid::new_v4()));
    assert!(matches!(
        svc.admit(with_source(
            company_id,
            version_id,
            "missing-parent",
            TriggerSource::Child {
                parent: ChildCause::ToolCall(call)
            },
            trace
        ))
        .await,
        Err(AppError::BadRequest(_))
    ));
    assert!(store.state.lock().unwrap().runs.is_empty());
    store.state.lock().unwrap().fail_source = true;
    assert!(matches!(
        svc.admit(with_source(
            company_id,
            version_id,
            "reader-error",
            TriggerSource::Message { message_id },
            trace
        ))
        .await,
        Err(AppError::Database(_))
    ));
    assert!(store.state.lock().unwrap().jobs.is_empty());
}

#[tokio::test]
async fn child_triggers_are_refused_before_any_lookup() {
    // Category C (07-D1 §D1.2a): children are created only by their parent's runtime step, so
    // no cause admits one, real or forged. The failing source reader proves no lookup ran.
    let company_id = company();
    let version_id = version();
    let store = MemoryStore::new(owned(company_id, version_id, 256));
    let svc = service(&store);
    let AdmissionResult::Created(parent_run) = svc
        .admit(request(company_id, version_id, "parent", json!(1)))
        .await
        .unwrap()
    else {
        panic!("parent created")
    };
    let actual = store.state.lock().unwrap().jobs[0].step.execution().clone();
    let other_visit = ExecutionRef::new(
        company_id,
        parent_run,
        ExecutionId::new(Uuid::new_v4()),
        actual.step_id().clone(),
    );
    let call = ToolCallId::new(Uuid::new_v4());
    store.state.lock().unwrap().fail_source = true;
    let before = store.state.lock().unwrap().runs.len();
    let causes = [
        ChildCause::ToolCall(ChildToolCallRef::new(actual.clone(), call)),
        ChildCause::ToolCall(ChildToolCallRef::new(other_visit, call)),
        ChildCause::Step(ChildStepRef::new(actual, ChildOrdinal::FIRST)),
    ];
    for (index, parent) in causes.into_iter().enumerate() {
        let result = svc
            .admit(with_source(
                company_id,
                version_id,
                &format!("child-{index}"),
                TriggerSource::Child { parent },
                CorrelationId::new(),
            ))
            .await;
        assert!(matches!(result, Err(AppError::BadRequest(_))), "{result:?}");
    }
    assert_eq!(store.state.lock().unwrap().runs.len(), before);
}

#[test]
fn claim_constructor_rejects_disagreeing_run_and_step() {
    let company_id = company();
    let first = test_causality(company_id, RunId::new(Uuid::new_v4()));
    let second = test_causality(company_id, RunId::new(Uuid::new_v4()));
    let step = StepCausality::new(
        &first,
        ExecutionRef::new(
            company_id,
            first.run_id(),
            ExecutionId::new(Uuid::new_v4()),
            step_id("repeat"),
        ),
    )
    .unwrap();
    assert!(matches!(
        ClaimedExecution::new(
            second,
            step,
            WorkerId::new(Uuid::new_v4()),
            FenceToken::new(Uuid::new_v4()),
            OwnershipReceipt::new(Uuid::new_v4()),
            SystemTime::now(),
        ),
        Err(AppError::Internal(_))
    ));
}

#[tokio::test]
async fn correlation_does_not_authorize_or_fence() {
    let company_id = company();
    let version_id = version();
    let store = MemoryStore::new(owned(company_id, version_id, 256));
    let trace = CorrelationId::new();
    let mut denied = with_source(
        company_id,
        version_id,
        "denied",
        TriggerSource::Manual,
        trace,
    );
    denied.actor = WorkflowActor::authenticated(Uuid::new_v4()).unwrap();
    assert!(matches!(
        service(&store).admit(denied).await,
        Err(AppError::NotFound(_))
    ));
    assert!(store.state.lock().unwrap().runs.is_empty());
    let AdmissionResult::Created(run_id) = service(&store)
        .admit(with_source(
            company_id,
            version_id,
            "allowed",
            TriggerSource::Manual,
            trace,
        ))
        .await
        .unwrap()
    else {
        panic!("created")
    };
    let claims = store
        .claim_ready(ClaimRequest {
            worker_id: WorkerId::new(Uuid::new_v4()),
            batch: ClaimBatch::new(1).unwrap(),
            lease: LeaseDuration::new(std::time::Duration::from_secs(30)).unwrap(),
        })
        .await
        .unwrap();
    assert_eq!(claims[0].run_id(), run_id);
    assert_ne!(claims[0].fence.as_uuid(), trace.as_uuid());
    assert_ne!(claims[0].receipt.as_uuid(), trace.as_uuid());
}
