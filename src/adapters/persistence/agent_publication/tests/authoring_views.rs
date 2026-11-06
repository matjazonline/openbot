use super::*;
use crate::application::agent::{
    AgentAuthoringReads, AgentDraftPersistence, AgentGrantState, AgentToolAssignment,
    AgentToolChoice, SaveCompanyAgent,
};

#[tokio::test]
async fn autonomous_agent_views_authoring_separates_active_and_draft_and_omits_credentials_and_active_prompt()
 {
    let f = Fixture::new().await;
    f.update(AgentWrite {
        system_prompt: Some("published-only-sensitive-marker".into()),
        ..write()
    })
    .await;
    let published =
        f.p.publish_agent(f.request("active-before-draft").await)
            .await
            .unwrap();
    let before = f.p.authoring_state(f.scope).await.unwrap();
    f.p.save_company_agent(SaveCompanyAgent {
        scope: f.scope,
        expected_revision: before.agent.draft_revision,
        write: AgentWrite {
            system_prompt: Some("Current editable draft".into()),
            ..write()
        },
        tools: Some(vec![AgentToolAssignment {
            binding: None,
            alias: AgentToolAlias::parse("new.send").unwrap(),
            choice: AgentToolChoice::Builtin {
                tool: AgentBuiltinTool::MessageSend,
            },
        }]),
    })
    .await
    .unwrap();
    let current = f.p.authoring_state(f.scope).await.unwrap();
    let active = current.active.as_ref().unwrap();
    assert_eq!(active.id, published.id);
    assert_eq!(active.draft_revision, published.draft_revision);
    assert_eq!(
        current.agent.draft_revision.get(),
        active.draft_revision.get() + 1
    );
    assert_eq!(active.tools[0].assignment.binding, Some(f.binding.id));
    assert_eq!(active.tools[0].access, AgentGrantState::Live);
    assert_eq!(current.tools[0].assignment.alias.as_str(), "new.send");
    assert_ne!(current.tools[0].assignment.binding, Some(f.binding.id));
    assert_eq!(current.history.items[0].id, published.id);
    let encoded = serde_json::to_string(&current).unwrap();
    assert!(encoded.contains("Current editable draft"));
    for secret in [
        "published-only-sensitive-marker",
        "fixture-only-model-key",
        "enc:v2:",
        "envelope",
    ] {
        assert!(!encoded.contains(secret));
    }
    f.p.revoke_agent_grant(f.scope, AgentGrantIdentity::Binding(f.binding.id))
        .await
        .unwrap();
    let revoked = f.p.authoring_state(f.scope).await.unwrap();
    let revoked = revoked.active.unwrap();
    assert_eq!(revoked.id, published.id);
    assert_eq!(revoked.tools[0].access, AgentGrantState::Revoked);
    assert_eq!(
        revoked.tools[0].assignment.choice,
        active.tools[0].assignment.choice
    );
}

async fn seed_history_ties(f: &Fixture, original: AgentRevisionId) {
    let mut tx = f.p.pool().begin().await.unwrap();
    sqlx::query("WITH copies AS (INSERT INTO agent_revisions(company_id,agent_id,id,draft_revision,content,digest,created_at) SELECT company_id,agent_id,gen_random_uuid(),draft_revision,content,digest,$4 FROM agent_revisions CROSS JOIN generate_series(1,45) AS sequence WHERE company_id=$1 AND agent_id=$2 AND id=$3 RETURNING id) INSERT INTO agent_revision_bindings(company_id,agent_id,revision_id,binding_id,position) SELECT member.company_id,member.agent_id,copies.id,member.binding_id,member.position FROM copies CROSS JOIN agent_revision_bindings AS member WHERE member.company_id=$1 AND member.agent_id=$2 AND member.revision_id=$3")
        .bind(f.scope.company.as_uuid()).bind(f.scope.agent.as_uuid()).bind(original.as_uuid())
        .bind(chrono::Utc::now()+chrono::Duration::seconds(1)).execute(&mut *tx).await.unwrap();
    tx.commit().await.unwrap();
}

#[tokio::test]
async fn autonomous_agent_views_history_pages_timestamp_ties_without_payloads_or_foreign_rows() {
    let f = Fixture::new().await;
    f.update(AgentWrite {
        system_prompt: Some("History must never return this prompt".into()),
        ..write()
    })
    .await;
    let original =
        f.p.publish_agent(f.request("history-base").await)
            .await
            .unwrap();
    seed_history_ties(&f, original.id).await;
    let mut before = None;
    let mut ids = Vec::new();
    let mut sizes = Vec::new();
    for _ in 0..3 {
        let page = f.p.revision_history(f.scope, before).await.unwrap();
        assert!(
            !serde_json::to_string(&page)
                .unwrap()
                .contains("History must never return this prompt")
        );
        sizes.push(page.items.len());
        ids.extend(page.items.iter().map(|item| item.id.as_uuid()));
        before = page.next;
    }
    assert_eq!(sizes, vec![20, 20, 6]);
    assert!(before.is_none());
    assert_eq!(ids.last(), Some(&original.id.as_uuid()));
    assert!(ids[..45].windows(2).all(|pair| pair[0] > pair[1]));
    assert_eq!(
        ids.iter()
            .copied()
            .collect::<std::collections::BTreeSet<_>>()
            .len(),
        46
    );
    let other = Fixture::new().await;
    let foreign = AgentPublicationScope {
        agent: other.scope.agent,
        ..f.scope
    };
    assert!(matches!(
        f.p.revision_history(foreign, None).await,
        Err(AppError::NotFound(_))
    ));
    let unauthorized = AgentPublicationScope {
        actor: other.scope.actor,
        ..f.scope
    };
    assert!(matches!(
        f.p.authoring_state(unauthorized).await,
        Err(AppError::NotFound(_))
    ));
}
