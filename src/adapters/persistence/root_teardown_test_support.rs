//! Real-schema teardown payload and assertions shared by lifecycle regressions.
use super::{PostgresPersistence, department_use_cases};
use crate::{
    application::{
        knowledge::*,
        ports::{account_deletion::HostAccountDeletion, channel::*, department::DepartmentWrite},
    },
    domain::workflow::CompanyId,
    entities::{department::*, transport::PrincipalId},
    use_cases::company::{CompanyPersistence, CompanyWrite},
};
use serde_json::Value;
use uuid::Uuid;

#[path = "root_identity_fk_test_support.rs"]
pub(crate) mod identities;

#[path = "root_attribution_tombstone_test_support.rs"]
mod attribution;
#[path = "root_dispatch_fk_test_support.rs"]
pub(crate) mod dispatch;
#[path = "root_native_fk_test_support.rs"]
pub(crate) mod native;

#[derive(Clone, Copy)]
pub(crate) enum Teardown {
    Company,
    Account,
}

pub(super) enum FkExpectation {
    Valid,
    Refused(&'static str),
}

pub(super) async fn finish_fk_probe(
    mut tx: sqlx::Transaction<'_, sqlx::Postgres>,
    expected: FkExpectation,
) {
    match expected {
        FkExpectation::Valid => {
            sqlx::query("SET CONSTRAINTS ALL IMMEDIATE")
                .execute(&mut *tx)
                .await
                .unwrap();
        }
        FkExpectation::Refused(name) => {
            let error = sqlx::query(&format!("SET CONSTRAINTS \"{name}\" IMMEDIATE"))
                .execute(&mut *tx)
                .await
                .unwrap_err();
            let database = error.as_database_error().expect("a database FK refusal");
            assert_eq!(database.code().as_deref(), Some("23503"));
            assert_eq!(database.constraint(), Some(name));
        }
    }
    tx.rollback().await.unwrap();
}

async fn tree(p: &PostgresPersistence, company: CompanyId, owner: Uuid) -> DepartmentId {
    let departments = department_use_cases(p);
    let root: Uuid =
        sqlx::query_scalar("SELECT id FROM departments WHERE company_id=$1 AND is_root")
            .bind(company.as_uuid())
            .fetch_one(p.pool())
            .await
            .unwrap();
    let principal: Uuid = sqlx::query_scalar("SELECT principal_id FROM membership_of($1,$2)")
        .bind(owner)
        .bind(company.as_uuid())
        .fetch_one(p.pool())
        .await
        .unwrap();
    let mut parent = DepartmentId::new(root);
    for slug in ["teardown-branch", "teardown-leaf"] {
        let department = departments
            .create_department(
                owner,
                company.as_uuid(),
                DepartmentWrite {
                    name: slug.into(),
                    slug: slug.into(),
                    parent_id: Some(parent),
                    ..Default::default()
                },
                None,
            )
            .await
            .unwrap();
        assert_eq!(department.parent_id, Some(parent));
        departments
            .add_member(
                owner,
                company.as_uuid(),
                department.id,
                PrincipalId::new(principal),
            )
            .await
            .unwrap();
        departments
            .assign_role(
                owner,
                company.as_uuid(),
                department.id,
                DepartmentRole::Reviewer,
                PrincipalId::new(principal),
            )
            .await
            .unwrap();
        parent = department.id;
    }
    parent
}

fn document() -> PreparedKnowledgeDocument {
    let mut embedding = [0.0; 384];
    embedding[0] = 1.0;
    PreparedKnowledgeDocument {
        file: KnowledgeUploadFile::from_upload(
            "5_teardown.md",
            b"Retained tenant document".to_vec(),
        )
        .unwrap(),
        embedding_model: "test-model".into(),
        chunks: vec![EmbeddedKnowledgeChunk {
            chunk: KnowledgeChunk {
                heading: None,
                body: "Retained tenant document".into(),
            },
            embedding,
        }],
    }
}

pub(crate) async fn tree_and_document(p: &PostgresPersistence, company: CompanyId, owner: Uuid) {
    let leaf = tree(p, company, owner).await;
    let desk = crate::adapters::persistence::test_support::create_channel(
        p,
        company.as_uuid(),
        ChannelWrite {
            name: "Restricted teardown desk".into(),
            slug: "teardown-restricted".into(),
            enabled: true,
            department_id: Some(leaf),
            ..Default::default()
        },
    )
    .await
    .unwrap();
    assert_eq!(desk.department_id, leaf);
    assert_eq!(
        desk.access_mode,
        crate::entities::channel::ChannelAccessMode::Members
    );
    let documents = KnowledgeDocuments::replace(
        p,
        KnowledgeScope {
            company,
            actor_id: owner,
        },
        &[document()],
    )
    .await
    .unwrap();
    assert_eq!(documents.len(), 1);
    let chunks: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM knowledge_chunks WHERE company_id=$1 AND document_id=$2",
    )
    .bind(company.as_uuid())
    .bind(documents[0].id)
    .fetch_one(p.pool())
    .await
    .unwrap();
    assert_eq!(chunks, 1, "the fixture owns actual document chunks");
}

pub(super) async fn foreign_company(p: &PostgresPersistence, owner: Uuid) -> CompanyId {
    CompanyId::new(
        CompanyPersistence::create(
            p,
            owner,
            CompanyWrite {
                name: "Foreign FK control".into(),
                slug: format!("foreign-fk-{}", Uuid::new_v4().simple()),
                ..Default::default()
            },
        )
        .await
        .unwrap()
        .id,
    )
}

async fn installations(p: &PostgresPersistence, company: CompanyId) -> Vec<Value> {
    sqlx::query_scalar("SELECT to_jsonb(reservation) FROM local_installations AS reservation WHERE company_id=$1 ORDER BY installation_id")
        .bind(company.as_uuid()).fetch_all(p.pool()).await.unwrap()
}

async fn no_live_rows(p: &PostgresPersistence, company: CompanyId) {
    let tables: Vec<String> = sqlx::query_scalar(
        "SELECT columns.table_name FROM information_schema.columns AS columns \
         JOIN information_schema.tables AS tables USING(table_catalog,table_schema,table_name) \
         WHERE columns.table_schema='public' AND columns.column_name='company_id' \
           AND tables.table_type='BASE TABLE' \
           AND columns.table_name NOT IN ('local_installations', 'attribution_company_lifecycles', 'attribution_lifecycle_outbox') \
         ORDER BY columns.table_name LIMIT 501",
    )
    .fetch_all(p.pool())
    .await
    .unwrap();
    assert!(!tables.is_empty() && tables.len() <= 500);
    for table in tables {
        let identifier = table.replace('"', "\"\"");
        let statement = format!("SELECT count(*) FROM public.\"{identifier}\" WHERE company_id=$1");
        let count: i64 = sqlx::query_scalar(&statement)
            .bind(company.as_uuid())
            .fetch_one(p.pool())
            .await
            .unwrap();
        assert_eq!(count, 0, "company teardown left live rows in {table}");
    }
}

impl Teardown {
    pub(crate) async fn remove(self, p: &PostgresPersistence, company: CompanyId, owner: Uuid) {
        let reservations = installations(p, company).await;
        let attribution = attribution::Snapshot::capture(p, company).await;
        match self {
            Self::Company => CompanyPersistence::delete_for_user(p, owner, company.as_uuid())
                .await
                .unwrap(),
            Self::Account => p.delete_user_account(owner).await.unwrap(),
        }
        let exists: bool = sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM companies WHERE id=$1)")
            .bind(company.as_uuid())
            .fetch_one(p.pool())
            .await
            .unwrap();
        assert!(!exists);
        let account_exists: bool =
            sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM users WHERE id=$1)")
                .bind(owner)
                .fetch_one(p.pool())
                .await
                .unwrap();
        assert_eq!(account_exists, matches!(self, Self::Company));
        assert_eq!(
            installations(p, company).await,
            reservations,
            "global installation reservations retain exact identities and immutable content"
        );
        attribution.assert_deleted(p, company).await;
        no_live_rows(p, company).await;
    }
}
