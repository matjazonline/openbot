use super::*;

#[tokio::test]
async fn autonomous_agent_publication_preview_and_publish_without_a_workflow() {
    let f = Fixture::new().await;
    let preview = f.p.preview_agent(f.scope).await.unwrap();
    let published = f.p.publish_agent(f.request("first").await).await.unwrap();
    assert_eq!(
        published.definition.canonical_content().unwrap(),
        preview.definition.canonical_content().unwrap()
    );
    assert_eq!(published.draft_revision, preview.draft_revision);
    assert_eq!(
        f.p.active_agent_revision(f.scope)
            .await
            .unwrap()
            .unwrap()
            .id,
        published.id
    );
    assert_eq!(
        f.counts().await,
        PublicationCounts {
            revisions: 1,
            requests: 1
        }
    );
    let workflows: i64 =
        sqlx::query_scalar("SELECT count(*) FROM workflow_definitions WHERE company_id=$1")
            .bind(f.scope.company.as_uuid())
            .fetch_one(f.p.pool())
            .await
            .unwrap();
    assert_eq!(workflows, 0);
    let content = String::from_utf8(published.definition.canonical_content().unwrap()).unwrap();
    assert!(!content.contains("fixture-only-model-key"));
    assert!(!content.contains("api_key"));
}

#[tokio::test]
async fn autonomous_agent_publication_replay_returns_original_after_newer_activation() {
    let f = Fixture::new().await;
    let original = f.request("original").await;
    let first = f.p.publish_agent(original.clone()).await.unwrap();
    f.update(AgentWrite {
        system_prompt: Some("New agent instructions.".into()),
        ..write()
    })
    .await;
    let second = f.p.publish_agent(f.request("second").await).await.unwrap();
    let replay = f.p.publish_agent(original.clone()).await.unwrap();
    assert_eq!(replay.id, first.id);
    assert_eq!(replay.digest, first.digest);
    assert_eq!(
        replay.definition.instructions,
        "Original agent instructions."
    );
    assert_eq!(
        f.p.active_agent_revision(f.scope)
            .await
            .unwrap()
            .unwrap()
            .id,
        second.id
    );
    let mut conflicting = original;
    conflicting.expected_active_revision = Some(second.id);
    assert!(matches!(
        f.p.publish_agent(conflicting).await,
        Err(AppError::Conflict(_))
    ));
    assert_eq!(
        f.counts().await,
        PublicationCounts {
            revisions: 2,
            requests: 2
        }
    );
}

#[tokio::test]
async fn autonomous_agent_publication_competing_publishers_choose_one_without_partial_state() {
    let f = Fixture::new().await;
    let first = f.request("first").await;
    let second = f.request("second").await;
    let (one, two) = tokio::join!(f.p.publish_agent(first), f.p.publish_agent(second));
    let winner = match (one, two) {
        (Ok(winner), Err(AppError::Conflict(_))) | (Err(AppError::Conflict(_)), Ok(winner)) => {
            winner
        }
        _ => panic!("exactly one competing publication must commit"),
    };
    assert_eq!(
        f.p.active_agent_revision(f.scope)
            .await
            .unwrap()
            .unwrap()
            .id,
        winner.id
    );
    assert_eq!(
        f.counts().await,
        PublicationCounts {
            revisions: 1,
            requests: 1
        }
    );
}

#[tokio::test]
async fn autonomous_agent_publication_competing_identical_keys_return_one_result() {
    let f = Fixture::new().await;
    let request = f.request("same-key").await;
    let (one, two) = tokio::join!(
        f.p.publish_agent(request.clone()),
        f.p.publish_agent(request)
    );
    assert_eq!(one.unwrap().id, two.unwrap().id);
    assert_eq!(
        f.counts().await,
        PublicationCounts {
            revisions: 1,
            requests: 1
        }
    );
}

#[tokio::test]
async fn autonomous_agent_publication_validation_failure_preserves_active_and_history() {
    let f = Fixture::new().await;
    let first = f.p.publish_agent(f.request("valid").await).await.unwrap();
    f.update(AgentWrite {
        model: Some("not-an-offered-model".into()),
        ..write()
    })
    .await;
    assert!(f.p.publish_agent(f.request("invalid").await).await.is_err());
    assert_eq!(
        f.p.active_agent_revision(f.scope)
            .await
            .unwrap()
            .unwrap()
            .id,
        first.id
    );
    assert_eq!(
        f.p.published_agent_revision(f.scope, first.id)
            .await
            .unwrap()
            .unwrap()
            .digest,
        first.digest
    );
    assert_eq!(
        f.counts().await,
        PublicationCounts {
            revisions: 1,
            requests: 1
        }
    );
}

#[tokio::test]
async fn autonomous_agent_publication_removal_revokes_identity_and_new_identity_cannot_revive_it() {
    let f = Fixture::new().await;
    let first =
        f.p.publish_agent(f.request("original").await)
            .await
            .unwrap();
    f.update(AgentWrite {
        tool_bindings: Some(vec![]),
        ..write()
    })
    .await;
    let live: bool = sqlx::query_scalar("SELECT revoked_at IS NULL FROM agent_binding_grants WHERE company_id=$1 AND agent_id=$2 AND binding_id=$3")
        .bind(f.scope.company.as_uuid()).bind(f.scope.agent.as_uuid()).bind(f.binding.id.as_uuid())
        .fetch_one(f.p.pool()).await.unwrap();
    assert!(
        live,
        "an ordinary draft edit cannot revoke a published grant"
    );
    f.p.publish_agent(f.request("removed").await).await.unwrap();
    f.update(AgentWrite {
        tool_bindings: Some(vec![f.binding.clone()]),
        ..write()
    })
    .await;
    assert!(matches!(
        f.p.publish_agent(f.request("retired").await).await,
        Err(AppError::Conflict(_))
    ));
    let fresh = AgentToolBinding::builtin(AgentBuiltinTool::MessageReply);
    f.update(AgentWrite {
        tool_bindings: Some(vec![fresh.clone()]),
        ..write()
    })
    .await;
    let current = f.p.publish_agent(f.request("fresh").await).await.unwrap();
    assert_eq!(current.definition.tools[0].binding.id, fresh.id);
    let old_revoked: bool = sqlx::query_scalar(
        "SELECT revoked_at IS NOT NULL FROM agent_binding_grants WHERE binding_id=$1",
    )
    .bind(f.binding.id.as_uuid())
    .fetch_one(f.p.pool())
    .await
    .unwrap();
    assert!(old_revoked);
    assert_eq!(
        f.p.published_agent_revision(f.scope, first.id)
            .await
            .unwrap()
            .unwrap()
            .digest,
        first.digest
    );
    assert_eq!(
        f.counts().await,
        PublicationCounts {
            revisions: 3,
            requests: 3
        }
    );
}

#[tokio::test]
async fn autonomous_agent_publication_revoke_deactivate_and_delete_preserve_frozen_result() {
    let f = Fixture::new().await;
    let request = f.request("original").await;
    let first = f.p.publish_agent(request.clone()).await.unwrap();
    let grant = AgentGrantIdentity::Binding(f.binding.id);
    f.p.revoke_agent_grant(f.scope, grant).await.unwrap();
    f.p.revoke_agent_grant(f.scope, grant).await.unwrap();
    assert!(matches!(
        f.p.deactivate_agent(f.scope, None).await,
        Err(AppError::Conflict(_))
    ));
    f.p.deactivate_agent(f.scope, Some(first.id)).await.unwrap();
    assert!(f.p.active_agent_revision(f.scope).await.unwrap().is_none());
    assert_eq!(f.p.publish_agent(request).await.unwrap().id, first.id);
    assert!(f.p.active_agent_revision(f.scope).await.unwrap().is_none());
    assert!(matches!(
        AgentPersistence::delete(&f.p, f.scope.agent.as_uuid()).await,
        Err(AppError::Conflict(_))
    ));
    assert_eq!(
        f.p.published_agent_revision(f.scope, first.id)
            .await
            .unwrap()
            .unwrap()
            .digest,
        first.digest
    );
}

#[tokio::test]
async fn autonomous_agent_publication_requires_current_authority_and_scopes_history() {
    let f = Fixture::new().await;
    let other = Fixture::new().await;
    let request = f.request("original").await;
    let first = f.p.publish_agent(request.clone()).await.unwrap();
    assert!(
        f.p.published_agent_revision(other.scope, first.id)
            .await
            .unwrap()
            .is_none()
    );
    let foreign = AgentPublicationScope {
        actor: other.scope.actor,
        ..f.scope
    };
    assert!(f.p.preview_agent(foreign).await.is_err());
    sqlx::query("DELETE FROM principals WHERE company_id=$1 AND user_id=$2")
        .bind(f.scope.company.as_uuid())
        .bind(f.scope.actor.user_id())
        .execute(f.p.pool())
        .await
        .unwrap();
    assert!(f.p.publish_agent(request).await.is_err());
    assert_eq!(
        f.counts().await,
        PublicationCounts {
            revisions: 1,
            requests: 1
        }
    );
}

#[tokio::test]
async fn autonomous_agent_publication_native_only_needs_no_hosted_connection() {
    let f = Fixture::new().await;
    f.update(AgentWrite {
        provider: None,
        model: None,
        tool_bindings: Some(vec![]),
        ..write()
    })
    .await;
    f.p.replace_model_connections_for_user(f.owner, f.scope.company.as_uuid(), vec![])
        .await
        .unwrap();
    let preview = f.p.preview_agent(f.scope).await.unwrap();
    assert!(preview.definition.model.is_none());
    let published =
        f.p.publish_agent(f.request("native-no-hosted-model").await)
            .await
            .unwrap();
    assert!(published.definition.model.is_none());
    assert_eq!(
        published.definition.content_hash().unwrap(),
        published.digest
    );
    let active = f.p.active_agent_revision(f.scope).await.unwrap().unwrap();
    assert_eq!(active.id, published.id);
    assert!(active.definition.model.is_none());
    assert!(
        crate::application::workflow::publication::ResolvedAgent::from_revision(
            crate::entities::agent_capability::DependencyManifestId::new(Uuid::new_v4()),
            active,
        )
        .is_err(),
        "hosted execution must refuse a native-only revision"
    );
}
