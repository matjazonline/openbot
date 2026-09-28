use super::*;
use crate::adapters::workflow_source::{WorkflowSourceDecoder, decode};
use crate::application::workflow::{WorkflowActor, compiler, registry};
use crate::domain::entities::company_member::CompanyMembership;
use crate::domain::workflow::{CompanyId, TemplateId, TemplateRevision, VersionId, WorkflowId};
use std::collections::BTreeMap;
use uuid::Uuid;
#[path = "test_support.rs"]
mod support;
use support::*;
#[path = "validation_tests.rs"]
mod validation;

#[tokio::test]
async fn workflow_templates_copy_snapshots_survive_update_withdrawal_and_company_isolation() {
    let store = Store::default();
    for company in [company(10), company(11)] {
        store.grant(company, actor(), CompanyMembership::Owner);
    }
    let example = registry::example("data.map").unwrap();
    let first = catalogue(&example, 1);
    let a = service(&store)
        .copy(&first, request(company(10), 1, 20))
        .await
        .unwrap();
    let b = service(&store)
        .copy(&first, request(company(11), 1, 21))
        .await
        .unwrap();
    assert_ne!(a.workflow(), b.workflow());
    assert_ne!(a.source(), b.source());
    assert_eq!(a.origin(), b.origin());
    assert_eq!(a.revision().get(), 1);
    assert_eq!(
        a.origin().unwrap().source_identity(),
        first.entries()[0].source_identity()
    );
    assert_ne!(a.source_identity(), a.origin().unwrap().source_identity());
    let mut edited = registry::example("data.map").unwrap();
    edited.source.push_str("\n# operator v2\n");
    let mut offered = offer(&edited, 2);
    offered.title = "Updated starter";
    offered.description = "Updated metadata";
    let second = TemplateCatalogue::build(&[offered], &WorkflowSourceDecoder).unwrap();
    let c = service(&store)
        .copy(&second, request(company(10), 2, 22))
        .await
        .unwrap();
    assert_eq!(c.title(), "Updated starter");
    assert_eq!(c.description(), "Updated metadata");
    assert_eq!(c.origin().unwrap().revision().get(), 2);
    assert!(c.source().ends_with("# operator v2\n"));
    assert_eq!(first.entries()[0].source(), example.source);
    drop(first);
    drop(second);
    let withdrawn = TemplateCatalogue::build(&[], &WorkflowSourceDecoder).unwrap();
    assert!(matches!(
        service(&store)
            .copy(&withdrawn, request(company(10), 2, 23))
            .await,
        Err(TemplateError::Missing)
    ));
    assert_eq!(store.0.lock().unwrap().drafts[&a.workflow()], a);
    assert_eq!(a.title(), "Starter");
    assert!(!a.source().contains("operator v2"));
    for draft in [a, b, c] {
        let parsed = compiler::parse_workflow(
            &decode(draft.source()).unwrap(),
            VersionId::new(Uuid::nil()),
        )
        .unwrap();
        assert_eq!(parsed.definition.workflow_id, draft.workflow());
    }
}

#[tokio::test]
async fn workflow_templates_real_membership_authorizes_before_lookup_or_preparation() {
    let example = registry::example("data.map").unwrap();
    let catalogue = catalogue(&example, 1);
    let store = Store::default();
    for (n, role) in [
        (30, CompanyMembership::Owner),
        (31, CompanyMembership::Admin),
    ] {
        store.grant(company(10), actor(), role);
        service(&store)
            .copy(&catalogue, request(company(10), 1, n))
            .await
            .unwrap();
    }
    for role in [
        Some(CompanyMembership::Member),
        Some(CompanyMembership::None),
        None,
    ] {
        store.0.lock().unwrap().membership.clear();
        if let Some(role) = role {
            store.grant(company(10), actor(), role);
        }
        // Stale revision and root reuse would fail later: authority wins first.
        assert!(matches!(
            service(&store)
                .copy(&catalogue, request(company(10), 9, 1))
                .await,
            Err(TemplateError::Application(AppError::NotFound(_)))
        ));
    }
    // Operator privileges are intentionally absent from company authorization.
    let operator_actor = WorkflowActor::authenticated(Uuid::from_u128(999)).unwrap();
    let req = CopyTemplateRequest::new(
        company(10),
        operator_actor,
        TemplateId::parse("missing").unwrap(),
        TemplateRevision::new(1).unwrap(),
        workflow(32),
    )
    .unwrap();
    assert!(matches!(
        service(&store).copy(&catalogue, req).await,
        Err(TemplateError::Application(AppError::NotFound(_)))
    ));
    store.grant(company(10), actor(), CompanyMembership::Owner);
    assert!(matches!(
        service(&store)
            .copy(&catalogue, request(company(11), 1, 33))
            .await,
        Err(TemplateError::Application(AppError::NotFound(_)))
    ));
    store.0.lock().unwrap().reader_error = true;
    assert!(matches!(
        service(&store)
            .copy(&catalogue, request(company(10), 1, 34))
            .await,
        Err(TemplateError::Application(AppError::Database(_)))
    ));
    assert_eq!(store.0.lock().unwrap().writes, 2);
}

#[tokio::test]
async fn workflow_templates_stale_missing_colliding_and_revoked_copies_do_not_overwrite() {
    let example = registry::example("data.map").unwrap();
    let catalogue = catalogue(&example, 1);
    let store = Store::default();
    store.grant(company(10), actor(), CompanyMembership::Owner);
    let service = service(&store);
    assert!(matches!(
        service.copy(&catalogue, request(company(10), 2, 30)).await,
        Err(TemplateError::StaleRevision)
    ));
    let mut missing = request(company(10), 1, 30);
    missing.template = TemplateId::parse("absent").unwrap();
    assert!(matches!(
        service.copy(&catalogue, missing).await,
        Err(TemplateError::Missing)
    ));
    assert!(matches!(
        service.copy(&catalogue, request(company(10), 1, 1)).await,
        Err(TemplateError::Validation(_))
    ));
    assert_eq!(store.0.lock().unwrap().writes, 0);
    let (a, b) = tokio::join!(
        service.copy(&catalogue, request(company(10), 1, 30)),
        service.copy(&catalogue, request(company(10), 1, 30))
    );
    assert_eq!(usize::from(a.is_ok()) + usize::from(b.is_ok()), 1);
    let (winner, loser) = if let Ok(a) = a {
        (a, b)
    } else {
        (b.unwrap(), a)
    };
    assert!(matches!(
        loser,
        Err(TemplateError::Application(AppError::Conflict(_)))
    ));
    assert_eq!(store.0.lock().unwrap().drafts[&workflow(30)], winner);
    store.0.lock().unwrap().revoke_at_commit = true;
    assert!(matches!(
        service.copy(&catalogue, request(company(10), 1, 31)).await,
        Err(TemplateError::Application(AppError::NotFound(_)))
    ));
    assert_eq!(store.0.lock().unwrap().drafts.len(), 1);
}

#[tokio::test]
async fn workflow_templates_company_declarations_remain_unpublished_source_only() {
    for name in ["agent.run", "mcp.call", "workflow.call"] {
        let example = registry::example(name).unwrap();
        let catalogue = catalogue(&example, 1);
        let store = Store::default();
        store.grant(company(10), actor(), CompanyMembership::Admin);
        let draft = service(&store)
            .copy(&catalogue, request(company(10), 1, 80))
            .await
            .unwrap();
        // No fact bundle, grant, owned version or binding is inserted by this port.
        assert_eq!(store.0.lock().unwrap().drafts.len(), 1);
        if name != "agent.run" {
            let error = registry::compile(
                decode(draft.source()).unwrap(),
                VersionId::new(Uuid::nil()),
                &registry::CatalogueFacts::default(),
                &BTreeMap::new(),
            )
            .err()
            .unwrap();
            assert!(!error.code.is_empty());
        }
    }
}
