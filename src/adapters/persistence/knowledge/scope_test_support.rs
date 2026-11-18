use super::*;
use crate::{
    application::ports::department::DepartmentWrite,
    entities::{
        department::{DepartmentId, DepartmentRole},
        transport::PrincipalId,
    },
};
use std::collections::BTreeSet;

pub(super) async fn unit(f: &Fixture, name: &str, parent: Option<DepartmentId>) -> DepartmentId {
    crate::adapters::persistence::test_support::department_use_cases(&f.store)
        .create_department(
            f.owner.actor_id,
            f.owner.company.as_uuid(),
            DepartmentWrite {
                name: name.into(),
                slug: name.to_lowercase(),
                parent_id: parent,
                ..Default::default()
            },
            None,
        )
        .await
        .unwrap()
        .id
}

pub(super) async fn person(f: &Fixture) -> PrincipalId {
    let id = sqlx::query_scalar(
        "SELECT id FROM principals WHERE company_id=$1 AND user_id=$2 AND kind='person'",
    )
    .bind(f.member.company.as_uuid())
    .bind(f.member.actor_id)
    .fetch_one(&f.pool)
    .await
    .unwrap();
    PrincipalId::new(id)
}

pub(super) async fn grant(f: &Fixture, unit: DepartmentId, role: DepartmentRole) {
    crate::adapters::persistence::test_support::department_use_cases(&f.store)
        .assign_role(
            f.owner.actor_id,
            f.owner.company.as_uuid(),
            unit,
            role,
            person(f).await,
        )
        .await
        .unwrap();
}

pub(super) async fn without_placement(f: &Fixture) {
    crate::adapters::persistence::test_support::department_use_cases(&f.store)
        .clear_role(
            f.owner.actor_id,
            f.owner.company.as_uuid(),
            f.root,
            DepartmentRole::Reviewer,
            person(f).await,
        )
        .await
        .unwrap();
}

pub(super) fn target(scope: KnowledgeScope, unit: DepartmentId) -> KnowledgeUploadTarget {
    KnowledgeUploadTarget { scope, unit }
}

pub(super) async fn upload(
    f: &Fixture,
    unit: DepartmentId,
    name: &str,
    text: &str,
) -> KnowledgeDocumentLayer {
    f.store
        .replace(target(f.owner, unit), &[prepared(name, text)])
        .await
        .unwrap()
        .remove(0)
}

pub(super) fn units(layers: &[KnowledgeDocumentLayer]) -> BTreeSet<DepartmentId> {
    layers.iter().map(|layer| layer.unit_id).collect()
}

pub(super) async fn manual(
    f: &Fixture,
    scope: KnowledgeScope,
) -> Vec<crate::application::knowledge::search::StoredKnowledgeChunk> {
    f.store
        .search_manual(ManualKnowledgeQuery {
            scope,
            unit: None,
            model: "test-model".into(),
            exact_terms: Vec::new(),
        })
        .await
        .unwrap()
}

pub(super) struct Group {
    pub(super) source: DepartmentId,
    pub(super) target: DepartmentId,
    pub(super) public: KnowledgeDocumentLayer,
    pub(super) private: KnowledgeDocumentLayer,
}

pub(super) async fn group(f: &Fixture) -> Group {
    let source = unit(f, "Source", None).await;
    let target = unit(f, "Target", None).await;
    let public = upload(f, source, "0_notes.md", "public").await;
    let private = upload(f, source, "5_notes.md", "private").await;
    Group {
        source,
        target,
        public,
        private,
    }
}

pub(super) fn move_command(
    scope: KnowledgeScope,
    anchor: &KnowledgeDocumentLayer,
    target: DepartmentId,
) -> MoveKnowledgeDocument {
    MoveKnowledgeDocument {
        scope,
        document_id: anchor.id,
        expected_revision: anchor.revision,
        target,
    }
}
