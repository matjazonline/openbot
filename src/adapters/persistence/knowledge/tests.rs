use super::*;
use crate::{
    adapters::persistence::test_support::own_database,
    domain::workflow::CompanyId,
    entities::user::User,
    use_cases::{company::CompanyWrite, user::UserPersistence},
};
use sqlx::PgPool;
use std::sync::Arc;
use uuid::Uuid;
mod constraints;
#[path = "delegated_scope_tests.rs"]
mod delegated_scopes;
mod races;
#[path = "scope_race_tests.rs"]
mod scope_races;
#[path = "scope_test_support.rs"]
mod scope_support;
#[path = "scope_tests.rs"]
mod scopes;
#[path = "search_tests.rs"]
mod search;
#[path = "search_quota_tests.rs"]
mod search_quota;
struct Fixture {
    pool: PgPool,
    store: Arc<PostgresPersistence>,
    owner: KnowledgeScope,
    member: KnowledgeScope,
    root: crate::entities::department::DepartmentId,
}
impl Fixture {
    async fn new(pool: PgPool) -> Self {
        let store = Arc::new(PostgresPersistence::new(pool.clone()));
        let owner = account(&store).await;
        let member = account(&store).await;
        let company = crate::use_cases::company::CompanyPersistence::create(
            store.as_ref(),
            owner.id,
            CompanyWrite {
                name: "Knowledge".into(),
                slug: format!("knowledge-{}", Uuid::new_v4()),
                ..Default::default()
            },
        )
        .await
        .unwrap();
        crate::adapters::persistence::test_support::insert_root_member(
            &pool,
            crate::domain::workflow::CompanyId::new(company.id),
            member.id,
            crate::entities::company_member::CompanyAccessRole::Member,
            crate::entities::access_level::AccessLevel::try_from(5_i16).unwrap(),
        )
        .await;
        let root: Uuid =
            sqlx::query_scalar("SELECT id FROM departments WHERE company_id=$1 AND is_root")
                .bind(company.id)
                .fetch_one(&pool)
                .await
                .unwrap();
        // These existing level/quota fixtures need an explicit placement, separate from root clearance.
        sqlx::query("INSERT INTO department_roles (company_id,department_id,role,principal_id) SELECT $1,$2,'reviewer',id FROM principals WHERE company_id=$1 AND user_id=$3")
            .bind(company.id).bind(root).bind(member.id).execute(&pool).await.unwrap();
        Self {
            pool,
            store,
            owner: KnowledgeScope {
                company: CompanyId::new(company.id),
                actor_id: owner.id,
            },
            member: KnowledgeScope {
                company: CompanyId::new(company.id),
                actor_id: member.id,
            },
            root: crate::entities::department::DepartmentId::new(root),
        }
    }
    fn target(&self, scope: KnowledgeScope) -> KnowledgeUploadTarget {
        KnowledgeUploadTarget {
            scope,
            unit: self.root,
        }
    }
    async fn replace(
        &self,
        scope: KnowledgeScope,
        name: &str,
        text: &str,
    ) -> KnowledgeDocumentLayer {
        self.store
            .replace(self.target(scope), &[prepared(name, text)])
            .await
            .unwrap()
            .remove(0)
    }
    async fn content(&self, id: Uuid) -> String {
        sqlx::query_scalar("SELECT content FROM knowledge_documents WHERE id=$1")
            .bind(id)
            .fetch_one(&self.pool)
            .await
            .unwrap()
    }
}
async fn account(store: &PostgresPersistence) -> User {
    let id = Uuid::new_v4();
    store
        .create_user(
            &format!("u{id}"),
            &format!("{id}@example.test"),
            "test-hash",
        )
        .await
        .unwrap()
}
fn prepared(name: &str, text: &str) -> PreparedKnowledgeDocument {
    let mut embedding = [0.0; 384];
    embedding[0] = 1.0;
    PreparedKnowledgeDocument {
        file: KnowledgeUploadFile::from_upload(name, text.as_bytes().to_vec()).unwrap(),
        embedding_model: "test-model".into(),
        chunks: vec![EmbeddedKnowledgeChunk {
            chunk: KnowledgeChunk {
                heading: Some("Heading".into()),
                body: text.into(),
            },
            embedding,
        }],
    }
}
#[tokio::test]
async fn knowledge_upload_layers_unicode_separator_override_and_duplicate_batch_have_matching_final_chunks()
 {
    let Some(db) = own_database().await else {
        return;
    };
    let f = Fixture::new(db.pool.clone()).await;
    let public = f.replace(f.owner, "0_STRASSE.md", "public").await;
    let high = f.replace(f.owner, "6_Straße.md", "secret").await;
    assert_ne!(public.id, high.id);
    let replaced = f.replace(f.owner, "6-STRASSE.md", "second").await;
    assert_eq!(high.id, replaced.id);
    assert_eq!(replaced.revision, 2);
    let final_rows = f
        .store
        .replace(
            f.target(f.owner),
            &[
                prepared("6_straße.md", "third"),
                prepared("6-STRASSE.md", "fourth"),
            ],
        )
        .await
        .unwrap();
    assert_eq!(final_rows.len(), 1);
    assert_eq!(final_rows[0].revision, 4);
    assert_eq!(f.content(high.id).await, "fourth");
    let body: String = sqlx::query_scalar("SELECT body FROM knowledge_chunks WHERE document_id=$1")
        .bind(high.id)
        .fetch_one(&f.pool)
        .await
        .unwrap();
    assert_eq!(body, "fourth");
    let visible = f.store.list(f.member, None).await.unwrap();
    assert_eq!(visible.layers.len(), 1);
    assert_eq!(visible.layers[0].id, public.id);
    assert!(matches!(
        f.store
            .replace(f.target(f.member), &[prepared("7_notes.md", "private")])
            .await,
        Err(AppError::Forbidden(_))
    ));
    assert!(matches!(
        f.store
            .delete(DeleteKnowledgeLayer {
                scope: f.member,
                document_id: high.id,
                expected_revision: 4
            })
            .await,
        Err(AppError::NotFound(_))
    ));
    assert!(matches!(
        f.store
            .delete(DeleteKnowledgeLayer {
                scope: f.owner,
                document_id: high.id,
                expected_revision: 3
            })
            .await,
        Err(AppError::Conflict(_))
    ));
    f.store
        .delete(DeleteKnowledgeLayer {
            scope: f.owner,
            document_id: high.id,
            expected_revision: 4,
        })
        .await
        .unwrap();
    assert_eq!(
        sqlx::query_scalar::<_, i64>("SELECT count(*) FROM knowledge_chunks WHERE document_id=$1")
            .bind(high.id)
            .fetch_one(&f.pool)
            .await
            .unwrap(),
        0
    );
}
#[tokio::test]
async fn knowledge_upload_authority_and_resource_ids_are_scoped_and_invalid_vectors_roll_back_every_file()
 {
    let Some(db) = own_database().await else {
        return;
    };
    let f = Fixture::new(db.pool.clone()).await;
    let other = Fixture::new(db.pool.clone()).await;
    let layer = f.replace(f.owner, "0_notes.md", "before").await;
    let foreign = KnowledgeScope {
        company: f.owner.company,
        actor_id: other.owner.actor_id,
    };
    assert!(matches!(
        f.store.access(foreign).await,
        Err(AppError::NotFound(_))
    ));
    assert!(matches!(
        f.store.list(foreign, None).await,
        Err(AppError::NotFound(_))
    ));
    assert!(matches!(
        f.store
            .preflight_upload(f.target(foreign), &[prepared("0_x.md", "x").file.size()])
            .await,
        Err(AppError::NotFound(_))
    ));
    assert!(matches!(
        f.store
            .replace(f.target(foreign), &[prepared("0_x.md", "x")])
            .await,
        Err(AppError::NotFound(_))
    ));
    assert!(matches!(
        f.store
            .delete(DeleteKnowledgeLayer {
                scope: other.owner,
                document_id: layer.id,
                expected_revision: 1
            })
            .await,
        Err(AppError::NotFound(_))
    ));
    let mut invalid = prepared("0_invalid.md", "bad");
    invalid.chunks[0].embedding = [0.0; 384];
    assert!(
        f.store
            .replace(
                f.target(f.owner),
                &[prepared("0_notes.md", "after"), invalid]
            )
            .await
            .is_err()
    );
    assert_eq!(f.content(layer.id).await, "before");
    assert_eq!(f.store.list(f.owner, None).await.unwrap().layers.len(), 1);
}

#[tokio::test]
async fn knowledge_upload_company_deletion_cascades_documents_and_chunks_without_deleting_the_uploader()
 {
    let Some(db) = own_database().await else {
        return;
    };
    let f = Fixture::new(db.pool.clone()).await;
    let layer = f.replace(f.member, "5_notes.md", "text").await;
    crate::use_cases::company::CompanyPersistence::delete(
        f.store.as_ref(),
        f.owner.company.as_uuid(),
    )
    .await
    .unwrap();
    for table in ["knowledge_documents", "knowledge_chunks"] {
        let query = format!("SELECT count(*) FROM {table} WHERE company_id=$1");
        assert_eq!(
            sqlx::query_scalar::<_, i64>(&query)
                .bind(f.owner.company.as_uuid())
                .fetch_one(&f.pool)
                .await
                .unwrap(),
            0
        );
    }
    assert!(
        crate::use_cases::user::UserPersistence::get_by_id(f.store.as_ref(), f.member.actor_id)
            .await
            .unwrap()
            .is_some()
    );
    assert!(matches!(
        f.store
            .delete(DeleteKnowledgeLayer {
                scope: f.member,
                document_id: layer.id,
                expected_revision: 1
            })
            .await,
        Err(AppError::NotFound(_))
    ));
}
