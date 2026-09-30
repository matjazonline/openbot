//! Authentic admitted associations and revocation across released verifier locks.
use super::proof_tests::all_tables;
use super::*;
use crate::adapters::persistence::workflow::tests::admission_tests::admission_history_tests::Conversation;
use crate::application::use_cases::channel::{ChannelPersistence, ChannelWrite};
use crate::application::workflow::{
    RelatedChannelId, RelatedThreadId, WorkflowAuthorization, WorkflowOperation,
};

struct Associated {
    fixture: AdmissionFixture,
    request: ActionDispatchRequest,
    verifier: Arc<ObservedVerifier>,
    association: RelatedAssociation,
    channel: Uuid,
    thread: Uuid,
    admin: WorkflowActor,
}

#[derive(Clone, Copy)]
enum AssociationKind {
    Channel,
    Thread,
}

async fn admitted_association(
    association: RelatedAssociation,
    f: &AdmissionFixture,
    key: &str,
) -> ActivationRequest {
    let mut admission = f.request(key, f.manual());
    admission.association = association;
    let command = f.prepare(admission).await;
    assert_eq!(
        f.persistence().admit(&command).await.unwrap(),
        AdmissionResult::Created(command.proposed_run_id())
    );
    let job = sqlx::query_scalar("SELECT id FROM background_tasks WHERE workflow_execution_id=$1")
        .bind(command.first_execution_id().as_uuid())
        .fetch_one(f.persistence().pool())
        .await
        .unwrap();
    ActivationRequest {
        company: command.company_id(),
        run: command.proposed_run_id(),
        execution: command.first_execution_id(),
        job: WorkflowJobId(job),
    }
}

async fn associated(kind: AssociationKind) -> Associated {
    // Use the real I/O step fixture: data.map is synchronous and has no I/O claim.
    let mut source: Value =
        serde_json::from_str(&registry::example("context.load").unwrap().source).unwrap();
    source["input_schema"] = json!({"type":"object","properties":{"value":{"type":"integer","minimum":1,"maximum":1000}},"required":["value"]});
    source["steps"]["start"]["with"]["max_tokens"] = json!({"ref":"/input/value"});
    let f = AdmissionFixture::with_binding(BindingFixture::from_source(source).await).await;
    let conversation = Conversation::new(&f).await;
    let association = match kind {
        AssociationKind::Thread => RelatedAssociation::Thread {
            channel_id: RelatedChannelId::new(conversation.channel),
            thread_id: RelatedThreadId::new(conversation.thread),
        },
        AssociationKind::Channel => {
            RelatedAssociation::Channel(RelatedChannelId::new(conversation.channel))
        }
    };
    let scope = admitted_association(association, &f, "associated").await;
    // Box the existing action fixture seam to keep nested real-owner setup on 2 MiB.
    let (f, request) = Box::pin(setup_action_on_fixture(
        f,
        scope,
        publication::ActionRecovery::Reconcile,
        json!({"value":1}),
    ))
    .await;
    resources(&f).await;
    let admin = principal(&f, Some("admin")).await;
    sqlx::query("UPDATE channels SET access_mode='allowlist' WHERE id=$1")
        .bind(conversation.channel)
        .execute(f.persistence().pool())
        .await
        .unwrap();
    sqlx::query("INSERT INTO channel_principal_grants(company_id,channel_id,principal_id,capability,provenance) SELECT $1,$2,id,'view','configured_allowlist' FROM principals WHERE company_id=$1 AND user_id=$3")
        .bind(scope.company.as_uuid()).bind(conversation.channel).bind(admin.user_id()).execute(f.persistence().pool()).await.unwrap();
    let verifier = Arc::new(ObservedVerifier::new(ledger(&f, &request).await));
    let provider = LedgerProvider::new(&f, Delivery::Pending);
    assert!(matches!(
        Box::pin(invoke(&f, &request, &provider)).await,
        Err(AppError::Timeout(_))
    ));
    park(&f, &request).await;
    Associated {
        fixture: f,
        request,
        verifier,
        association,
        channel: conversation.channel,
        thread: conversation.thread,
        admin,
    }
}

struct ForeignAssociations {
    company: CompanyId,
    association: RelatedAssociation,
    other: Conversation,
    scope: ActivationRequest,
}

async fn foreign_associations(f: &AdmissionFixture) -> ForeignAssociations {
    let foreign = CompanyPersistence::create(
        f.persistence(),
        f.binding.target.actor.user_id(),
        CompanyWrite {
            name: "Foreign".into(),
            slug: format!("foreign-{}", Uuid::new_v4().simple()),
            ..Default::default()
        },
    )
    .await
    .unwrap();
    let channel = ChannelPersistence::create(
        f.persistence(),
        foreign.id,
        ChannelWrite {
            name: "Foreign".into(),
            slug: "foreign".into(),
            ..Default::default()
        },
    )
    .await
    .unwrap();
    let foreign_thread = Uuid::new_v4();
    sqlx::query("INSERT INTO threads(id,company_id,channel_id,subject) VALUES($1,$2,$3,'Foreign')")
        .bind(foreign_thread)
        .bind(foreign.id)
        .bind(channel.id)
        .execute(f.persistence().pool())
        .await
        .unwrap();
    let other = Conversation::new(f).await;
    let other_scope = admitted_association(
        RelatedAssociation::Thread {
            channel_id: RelatedChannelId::new(other.channel),
            thread_id: RelatedThreadId::new(other.thread),
        },
        f,
        "other-run",
    )
    .await;
    ForeignAssociations {
        company: CompanyId::new(foreign.id),
        association: RelatedAssociation::Thread {
            channel_id: RelatedChannelId::new(channel.id),
            thread_id: RelatedThreadId::new(foreign_thread),
        },
        other,
        scope: other_scope,
    }
}

async fn assert_associations(
    f: &AdmissionFixture,
    association: RelatedAssociation,
    thread: Uuid,
    company: CompanyId,
    foreign_association: RelatedAssociation,
    other: &Conversation,
) {
    let p = f.persistence().clone();
    let authorization = LifecycleAuthorizer::new(p.clone(), p.clone(), p.clone());
    // Every identifier exists. The owner can authorize the genuine foreign pair.
    authorization
        .authorize(
            company,
            f.binding.target.actor,
            foreign_association,
            WorkflowOperation::Reconcile,
        )
        .await
        .unwrap();
    for bad in [
        match foreign_association {
            RelatedAssociation::Thread { channel_id, .. } => {
                RelatedAssociation::Channel(channel_id)
            }
            _ => unreachable!(),
        },
        foreign_association,
        RelatedAssociation::Thread {
            channel_id: RelatedChannelId::new(other.channel),
            thread_id: RelatedThreadId::new(thread),
        },
        RelatedAssociation::Thread {
            channel_id: match association {
                RelatedAssociation::Channel(id)
                | RelatedAssociation::Thread { channel_id: id, .. } => id,
                _ => unreachable!(),
            },
            thread_id: RelatedThreadId::new(other.thread),
        },
    ] {
        assert!(matches!(
            authorization
                .authorize(
                    f.binding.target.company,
                    f.binding.target.actor,
                    bad,
                    WorkflowOperation::Reconcile
                )
                .await,
            Err(AppError::NotFound(_))
        ));
    }
}

#[tokio::test]
async fn workflow_action_reconciliation_valid_foreign_associations_and_scopes_refuse() {
    for kind in [AssociationKind::Channel, AssociationKind::Thread] {
        let Associated {
            fixture: f,
            request,
            verifier,
            association,
            admin,
            thread,
            ..
        } = Box::pin(associated(kind)).await;
        let ForeignAssociations {
            company,
            association: foreign_association,
            other,
            scope: other_scope,
        } = foreign_associations(&f).await;
        assert_associations(
            &f,
            association,
            thread,
            company,
            foreign_association,
            &other,
        )
        .await;
        let before = all_tables(&f).await;
        let mut c = verified_command(&f, &request, &verifier, "valid-association").await;
        c.actor = admin;
        let reader = PostgresActionReconciliation::new(f.persistence().clone(), Resources);
        assert_eq!(reader.association(&c).await.unwrap(), association);
        for variant in 0..3 {
            let mut bad = c.clone();
            match variant {
                0 => {
                    bad.scope.company = company;
                    bad.actor = f.binding.target.actor;
                }
                1 => bad.scope.run = other_scope.run,
                _ => bad.scope.execution = other_scope.execution,
            }
            assert!(
                matches!(
                    Box::pin(execute(&f, &bad, verifier.clone())).await,
                    Err(AppError::NotFound(_))
                ),
                "valid foreign scope {variant}"
            );
            assert_eq!(all_tables(&f).await, before);
            assert_eq!(verifier.calls.load(Ordering::SeqCst), 0);
        }
        assert!(matches!(
            Box::pin(execute(&f, &c, verifier.clone()))
                .await
                .unwrap()
                .outcome,
            ReconciliationOutcome::UnknownRecorded
        ));
        assert_eq!(verifier.calls.load(Ordering::SeqCst), 1);
    }
}

#[tokio::test]
async fn workflow_action_reconciliation_paused_association_revocation_reauthorizes_replay() {
    for kind in [AssociationKind::Channel, AssociationKind::Thread] {
        let Associated {
            fixture: f,
            request,
            verifier: prepared,
            association,
            channel,
            admin,
            ..
        } = Box::pin(associated(kind)).await;
        let mut successful = verified_command(&f, &request, &prepared, "association-replay").await;
        successful.actor = admin;
        assert!(matches!(
            Box::pin(execute(&f, &successful, prepared.clone()))
                .await
                .unwrap()
                .outcome,
            ReconciliationOutcome::UnknownRecorded
        ));
        // The real provider barrier closes every old request; the paused verifier
        // now attests FinalNotApplied, so revoked access cannot commit a retry grant.
        barrier(&f, &request).await;
        let started = Arc::new(Notify::new());
        let release = Arc::new(Notify::new());
        let verifier = Arc::new(ObservedVerifier {
            ledger: prepared.ledger.clone(),
            calls: AtomicUsize::new(0),
            pause: Some((started.clone(), release.clone())),
        });
        let mut c = verified_command(&f, &request, &verifier, "association-revoked").await;
        c.actor = admin;
        let competitor = async {
            started.notified().await;
            tokio::time::timeout(Duration::from_secs(2), async {
                let changed=sqlx::query("DELETE FROM channel_principal_grants AS access_grant USING principals AS principal WHERE access_grant.company_id=$1 AND access_grant.channel_id=$2 AND principal.company_id=access_grant.company_id AND principal.id=access_grant.principal_id AND principal.user_id=$3")
                    .bind(c.scope.company.as_uuid()).bind(channel).bind(admin.user_id()).execute(f.persistence().pool()).await.unwrap();
                assert_eq!(changed.rows_affected(),1);
                all_tables(&f).await
            }).await.expect("snapshot channel/grant/run locks released before verifier I/O")
        };
        let settlement = async {
            let before = competitor.await;
            release.notify_one();
            before
        };
        let (result, before) =
            tokio::join!(Box::pin(execute(&f, &c, verifier.clone())), settlement);
        assert!(matches!(result, Err(AppError::NotFound(_))));
        assert_eq!(verifier.calls.load(Ordering::SeqCst), 1);
        assert_eq!(all_tables(&f).await, before);
        assert!(matches!(
            Box::pin(execute(&f, &successful, prepared.clone())).await,
            Err(AppError::NotFound(_))
        ));
        assert_eq!(
            prepared.calls.load(Ordering::SeqCst),
            1,
            "replay reauthorizes before verifier"
        );
        assert_eq!(all_tables(&f).await, before);
        assert_eq!(
            f.persistence()
                .head(c.scope.company, c.scope.run)
                .await
                .unwrap()
                .unwrap()
                .association,
            association
        );
        assert!(
            f.persistence()
                .claim_io(request.fence.scope, worker(), policy())
                .await
                .unwrap()
                .is_none()
        );
    }
}
