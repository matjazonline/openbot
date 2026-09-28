use super::*;
#[path = "admission_binding_tests.rs"]
mod admission_binding_tests;
#[path = "admission_tests.rs"]
mod admission_tests;
#[path = "binding_association_tests.rs"]
mod binding_association_tests;
#[path = "binding_schema_tests.rs"]
mod binding_schema_tests;
#[path = "binding_tests.rs"]
mod binding_tests;
#[path = "run_schema_tests.rs"]
mod run_schema_tests;
use crate::adapters::persistence::test_support::{OwnDatabase, own_database};
use crate::adapters::workflow_source::WorkflowSourceDecoder;
use crate::application::use_cases::{
    company::{CompanyPersistence, CompanyWrite},
    user::UserPersistence,
};

// Deliberately permissive preflight: these tests prove the SQL owner performs
// current authorization itself rather than trusting the service's earlier read.
#[derive(Clone)]
struct Preflight;
#[async_trait]
impl WorkflowAuthorization for Preflight {
    async fn authorize(
        &self,
        _: CompanyId,
        _: WorkflowActor,
        _: RelatedAssociation,
        _: WorkflowOperation,
    ) -> AppResult<()> {
        Ok(())
    }
}
#[async_trait]
impl PublicationDirectory for Preflight {
    async fn capture(
        &self,
        _: WorkflowActor,
        _: &CompanyWorkflowDraft,
    ) -> AppResult<PublicationDependencies> {
        Ok(PublicationDependencies {
            snapshots: DependencySnapshots::default(),
            children: vec![],
        })
    }
}
type Service = DefinitionService<Preflight, PostgresPersistence, WorkflowSourceDecoder, Preflight>;
struct Fixture {
    _db: OwnDatabase,
    persistence: PostgresPersistence,
    target: DefinitionTarget,
}
impl Fixture {
    async fn new() -> Self {
        let db = own_database()
            .await
            .expect("workflow SQL tests require a database");
        let persistence = PostgresPersistence::new(db.pool.clone());
        persistence
            .create_user("workflow_owner", "workflow@example.test", "hash")
            .await
            .unwrap();
        let user = persistence
            .get_by_email("workflow@example.test")
            .await
            .unwrap()
            .unwrap();
        let company = CompanyPersistence::create(
            &persistence,
            user.id,
            CompanyWrite {
                name: "Workflow tests".into(),
                slug: "workflow-tests".into(),
                ..Default::default()
            },
        )
        .await
        .unwrap();
        Self {
            _db: db,
            persistence,
            target: DefinitionTarget {
                company: CompanyId::new(company.id),
                actor: WorkflowActor::authenticated(user.id).unwrap(),
                workflow: WorkflowId::new(Uuid::from_u128(1)),
            },
        }
    }
    fn service(&self) -> Service {
        DefinitionService::new(
            Preflight,
            self.persistence.clone(),
            WorkflowSourceDecoder,
            Preflight,
        )
    }
    fn save(&self, expected: Option<DraftRevision>) -> SaveDraft {
        SaveDraft {
            target: self.target,
            expected,
            content: DraftContent {
                title: "Workflow".into(),
                description: "Description".into(),
                source: registry::example("data.map").unwrap().source,
            },
        }
    }
    fn publish(&self) -> PublishDraft {
        PublishDraft {
            target: self.target,
            expected: DraftRevision::new(1).unwrap(),
            version: VersionId::new(Uuid::new_v4()),
            key: IdempotencyKey::parse("publication").unwrap(),
        }
    }
}

#[tokio::test]
async fn workflow_sql_competing_edits_and_publication_replay() {
    let fixture = Fixture::new().await;
    let service = fixture.service();
    service.save(fixture.save(None)).await.unwrap();
    let publication = fixture.publish();
    let (first, second) = tokio::join!(
        service.publish(publication.clone()),
        service.publish(publication.clone())
    );
    let hash = first.unwrap().content_hash().as_str().to_owned();
    assert_eq!(second.unwrap().content_hash().as_str(), hash);
    let expected = DraftRevision::new(1).unwrap();
    let (first, second) = tokio::join!(
        service.save(fixture.save(Some(expected))),
        service.save(fixture.save(Some(expected)))
    );
    assert_eq!(usize::from(first.is_ok()) + usize::from(second.is_ok()), 1);
    service
        .archive(fixture.target, DraftRevision::new(2).unwrap())
        .await
        .unwrap();
    assert_eq!(
        service
            .publish(publication.clone())
            .await
            .unwrap()
            .content_hash()
            .as_str(),
        hash
    );
    assert!(
        fixture
            .persistence
            .selectable_version(fixture.target.company, publication.version)
            .await
            .unwrap()
            .is_none()
    );
    let count: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM workflow_definition_events WHERE event_kind = 'published'",
    )
    .fetch_one(fixture.persistence.pool())
    .await
    .unwrap();
    assert_eq!(count, 1);
}

#[tokio::test]
async fn workflow_sql_revocation_and_failed_final_statement_roll_back() {
    let fixture = Fixture::new().await;
    let service = fixture.service();
    sqlx::query(
        "ALTER TABLE workflow_definition_events ADD CONSTRAINT reject_test_event CHECK (false)",
    )
    .execute(fixture.persistence.pool())
    .await
    .unwrap();
    assert!(service.save(fixture.save(None)).await.is_err());
    assert!(
        fixture
            .persistence
            .draft(fixture.target.company, fixture.target.workflow)
            .await
            .unwrap()
            .is_none()
    );
    sqlx::query("ALTER TABLE workflow_definition_events DROP CONSTRAINT reject_test_event")
        .execute(fixture.persistence.pool())
        .await
        .unwrap();
    service.save(fixture.save(None)).await.unwrap();
    let mut revocation = fixture.persistence.pool().begin().await.unwrap();
    sqlx::query("DELETE FROM company_members WHERE company_id = $1 AND user_id = $2")
        .bind(fixture.target.company.as_uuid())
        .bind(fixture.target.actor.user_id())
        .execute(&mut *revocation)
        .await
        .unwrap();
    let (published, ()) = tokio::join!(service.publish(fixture.publish()), async {
        crate::adapters::persistence::test_support::wait_until_a_backend_is_blocked(
            fixture.persistence.pool(),
        )
        .await;
        revocation.commit().await.unwrap();
    });
    assert!(published.is_err());
    let count: i64 = sqlx::query_scalar("SELECT count(*) FROM workflow_versions")
        .fetch_one(fixture.persistence.pool())
        .await
        .unwrap();
    assert_eq!(count, 0);
}

#[tokio::test]
async fn workflow_sql_publish_archive_compete_and_publication_audit_failure_rolls_back() {
    let fixture = Fixture::new().await;
    let service = fixture.service();
    service.save(fixture.save(None)).await.unwrap();
    sqlx::query("ALTER TABLE workflow_definition_events ADD CONSTRAINT reject_publication CHECK (event_kind <> 'published')")
        .execute(fixture.persistence.pool()).await.unwrap();
    let request = fixture.publish();
    assert!(service.publish(request.clone()).await.is_err());
    assert!(
        fixture
            .persistence
            .publication(fixture.target.company, &request.key)
            .await
            .unwrap()
            .is_none()
    );
    sqlx::query("ALTER TABLE workflow_definition_events DROP CONSTRAINT reject_publication")
        .execute(fixture.persistence.pool())
        .await
        .unwrap();
    let mut blocker = fixture.persistence.pool().begin().await.unwrap();
    sqlx::query("SELECT id FROM companies WHERE id = $1 FOR UPDATE")
        .bind(fixture.target.company.as_uuid())
        .execute(&mut *blocker)
        .await
        .unwrap();
    let (published, archived, ()) = tokio::join!(
        service.publish(request.clone()),
        service.archive(fixture.target, DraftRevision::new(1).unwrap()),
        async {
            crate::adapters::persistence::test_support::wait_until_backends_are_blocked(
                fixture.persistence.pool(),
                2,
            )
            .await;
            blocker.commit().await.unwrap();
        }
    );
    archived.unwrap();
    let saved = fixture
        .persistence
        .publication(fixture.target.company, &request.key)
        .await
        .unwrap();
    assert_eq!(saved.is_some(), published.is_ok());
    assert!(
        fixture
            .persistence
            .selectable_version(fixture.target.company, request.version)
            .await
            .unwrap()
            .is_none()
    );
    assert!(
        service
            .publish(PublishDraft {
                key: IdempotencyKey::parse("after-archive").unwrap(),
                ..fixture.publish()
            })
            .await
            .is_err()
    );
}

#[tokio::test]
async fn workflow_sql_corrupt_source_fails_and_cross_company_version_is_rejected() {
    let fixture = Fixture::new().await;
    let service = fixture.service();
    service.save(fixture.save(None)).await.unwrap();
    let request = fixture.publish();
    service.publish(request.clone()).await.unwrap();
    let other = CompanyPersistence::create(
        &fixture.persistence,
        fixture.target.actor.user_id(),
        CompanyWrite {
            name: "Other".into(),
            slug: "other".into(),
            ..Default::default()
        },
    )
    .await
    .unwrap();
    let error = sqlx::query("INSERT INTO workflow_versions \
        (company_id, workflow_id, id, draft_revision, command_key, actor_id, title, description, source, bundle, content_hash) \
        SELECT $1, workflow_id, id, draft_revision, command_key, actor_id, title, description, source, bundle, content_hash \
        FROM workflow_versions WHERE company_id = $2")
        .bind(other.id).bind(fixture.target.company.as_uuid()).execute(fixture.persistence.pool()).await.unwrap_err();
    assert!(
        error
            .as_database_error()
            .unwrap()
            .is_foreign_key_violation()
    );
    sqlx::query(
        "UPDATE workflow_versions SET source = source || E'\\n# corrupt' WHERE company_id = $1",
    )
    .bind(fixture.target.company.as_uuid())
    .execute(fixture.persistence.pool())
    .await
    .unwrap();
    assert!(
        fixture
            .persistence
            .publication(fixture.target.company, &request.key)
            .await
            .is_err()
    );
    assert!(
        fixture
            .persistence
            .selectable_version(fixture.target.company, request.version)
            .await
            .is_err()
    );
}

#[tokio::test]
async fn workflow_sql_template_provenance_roundtrips_and_replay_reauthorizes() {
    let mut fixture = Fixture::new().await;
    fixture.target.workflow = WorkflowId::new(Uuid::new_v4());
    let example = registry::example("data.map").unwrap();
    let catalogue = TemplateCatalogue::build(
        &[TemplateOffer {
            id: TemplateId::parse("example").unwrap(),
            revision: TemplateRevision::new(1).unwrap(),
            title: "Example",
            description: "Description",
            source: &example.source,
            facts: &example.facts,
            dependencies: &std::collections::BTreeMap::from([(
                WorkflowId::new(Uuid::from_u128(1)),
                std::collections::BTreeSet::new(),
            )]),
        }],
        &WorkflowSourceDecoder,
    )
    .unwrap();
    let copied = TemplateCopyService::new(
        Preflight,
        fixture.persistence.clone(),
        WorkflowSourceDecoder,
    )
    .copy(
        &catalogue,
        CopyTemplateRequest::new(
            fixture.target.company,
            fixture.target.actor,
            TemplateId::parse("example").unwrap(),
            TemplateRevision::new(1).unwrap(),
            fixture.target.workflow,
        )
        .unwrap(),
    )
    .await
    .unwrap();
    let service = fixture.service();
    let mut edit = fixture.save(Some(copied.revision()));
    edit.content.source = copied.source().into();
    service.save(edit).await.unwrap();
    let saved = fixture
        .persistence
        .draft(fixture.target.company, fixture.target.workflow)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(saved.draft.origin(), copied.origin());
    let request = PublishDraft {
        expected: DraftRevision::new(2).unwrap(),
        ..fixture.publish()
    };
    service.publish(request.clone()).await.unwrap();
    let published = fixture
        .persistence
        .publication(fixture.target.company, &request.key)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(published.draft.origin(), copied.origin());
    sqlx::query("DELETE FROM company_members WHERE company_id = $1 AND user_id = $2")
        .bind(fixture.target.company.as_uuid())
        .bind(fixture.target.actor.user_id())
        .execute(fixture.persistence.pool())
        .await
        .unwrap();
    assert!(service.publish(request).await.is_err());
}
