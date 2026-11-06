use super::super::test_support::{Fixture, fields, form, text};
use super::*;

#[tokio::test]
async fn autonomous_agent_authoring_form_preserves_stale_input_tools_and_expected_revision() {
    let Some(f) = Fixture::new().await else {
        return;
    };
    let tools = r#"[{"binding":null,"alias":"message.reply","choice":{"kind":"builtin","tool":"message_reply"}}]"#;
    let submitted = form(&fields(f.agent.draft_revision, tools)).await;
    let saved = text(
        super::super::super::update_agent(
            f.workspace(f.f.owner.id),
            Path(f.agent.id),
            f.query(),
            Form(submitted),
        )
        .await
        .unwrap(),
    )
    .await;
    assert!(saved.contains("Edited &lt;agent&gt;"));
    assert!(
        saved.contains("Tools for this agent") && saved.contains("Saved draft and publication")
    );
    assert!(!saved.contains("authoring-secret-never-return"));
    let state = f.state().await;
    assert_eq!(state.tools.len(), 1);
    assert_eq!(
        state.agent.draft_revision.get(),
        f.agent.draft_revision.get() + 1
    );
    let original_binding = state.tools[0].assignment.binding;
    let stale_fields = fields(f.agent.draft_revision, tools);
    let stale = text(
        super::super::super::update_agent(
            f.workspace(f.f.owner.id),
            Path(f.agent.id),
            f.query(),
            Form(form(&stale_fields).await),
        )
        .await
        .unwrap(),
    )
    .await;
    assert!(
        stale.contains("Reload it before saving")
            && stale.contains("unsaved &lt;/textarea&gt;&lt;img"),
        "{stale}"
    );
    assert!(stale.contains(&format!(
        r#"name="expected_revision" value="{}""#,
        f.agent.draft_revision.get()
    )));
    assert!(stale.contains("&quot;message_reply&quot;"));
    assert!(!stale.contains("<img src=x onerror"));
    let unchanged = f.state().await;
    assert_eq!(unchanged.agent.draft_revision, state.agent.draft_revision);
    assert_eq!(unchanged.tools[0].assignment.binding, original_binding);
    assert!(unchanged.active.is_none());
}

#[tokio::test]
async fn autonomous_agent_authoring_preview_publish_replay_and_deactivation_omit_canonical_payloads()
 {
    let Some(f) = Fixture::new().await else {
        return;
    };
    let preview = text(
        preview_json(
            f.workspace(f.f.owner.id),
            Path((f.f.company.id, f.agent.id)),
        )
        .await
        .unwrap(),
    )
    .await;
    let preview: serde_json::Value = serde_json::from_str(&preview).unwrap();
    assert_eq!(preview["tool_count"], 0);
    let key = Uuid::new_v4().to_string();
    let payload = serde_json::json!({"expected_draft_revision":f.agent.draft_revision, "expected_active_revision":null,"key":key});
    let mut receipts = Vec::new();
    for _ in 0..2 {
        let receipt = text(
            publish_json(
                f.workspace(f.f.owner.id),
                Path((f.f.company.id, f.agent.id)),
                Json(serde_json::from_value(payload.clone()).unwrap()),
            )
            .await
            .unwrap(),
        )
        .await;
        assert!(
            !receipt.contains("private immutable instruction marker")
                && !receipt.contains("authoring-secret-never-return")
        );
        receipts.push(serde_json::from_str::<serde_json::Value>(&receipt).unwrap());
    }
    assert_eq!(receipts[0], receipts[1]);
    let state = f.state().await;
    assert_eq!(state.history.items.len(), 1);
    let active = state.active.unwrap().id;
    let deactivated = text(
        deactivate_json(
            f.workspace(f.f.owner.id),
            Path((f.f.company.id, f.agent.id)),
            Json(DeactivateInput {
                expected_active_revision: active,
            }),
        )
        .await
        .unwrap(),
    )
    .await;
    assert!(deactivated.contains("true"));
    assert!(f.state().await.active.is_none());
    assert_eq!(f.state().await.history.items.len(), 1);
}

#[tokio::test]
async fn autonomous_agent_authoring_failed_publication_retains_original_request_and_does_not_save_editor()
 {
    let Some(f) = Fixture::new().await else {
        return;
    };
    let tools = "[]";
    super::super::super::update_agent(
        f.workspace(f.f.owner.id),
        Path(f.agent.id),
        f.query(),
        Form(form(&fields(f.agent.draft_revision, tools)).await),
    )
    .await
    .unwrap();
    let key = AgentPublicationKey::parse(Uuid::new_v4().to_string()).unwrap();
    let html = publish_form(
        f.workspace(f.f.owner.id),
        Path(f.agent.id),
        f.query(),
        Form(PublicationForm {
            expected_draft_revision: f.agent.draft_revision,
            expected_active_revision: String::new(),
            key: key.clone(),
        }),
    )
    .await
    .unwrap()
    .0;
    assert!(
        html.contains("changed") && html.contains("original request key"),
        "{html}"
    );
    assert!(html.contains(&format!(r#"name="key" value="{}""#, key.as_str())));
    assert!(html.contains(&format!(
        r#"name="expected_draft_revision" value="{}""#,
        f.agent.draft_revision.get()
    )));
    assert!(html.contains(r#"name="expected_active_revision" value="""#));
    assert!(
        !html.contains(r#"name="system_prompt""#),
        "publication responses do not replace the editor"
    );
    let state = f.state().await;
    assert!(state.active.is_none() && state.history.items.is_empty());
    assert_eq!(
        state.agent.draft_revision.get(),
        f.agent.draft_revision.get() + 1
    );
}

#[tokio::test]
async fn autonomous_agent_authoring_exact_binding_revoke_and_foreign_mutation_refusal() {
    let Some(f) = Fixture::new().await else {
        return;
    };
    let tools = r#"[{"binding":null,"alias":"message.reply","choice":{"kind":"builtin","tool":"message_reply"}}]"#;
    super::super::super::update_agent(
        f.workspace(f.f.owner.id),
        Path(f.agent.id),
        f.query(),
        Form(form(&fields(f.agent.draft_revision, tools)).await),
    )
    .await
    .unwrap();
    let saved = f.state().await;
    let revision = f
        .workflow
        .agent_publications()
        .publish_agent(PublishAgent {
            scope: f.scope(),
            expected_draft_revision: saved.agent.draft_revision,
            expected_active_revision: None,
            key: AgentPublicationKey::parse(Uuid::new_v4().to_string()).unwrap(),
        })
        .await
        .unwrap();
    let binding = saved.tools[0].assignment.binding.unwrap();
    let live_panel = panel(f.workspace(f.f.owner.id), Path(f.agent.id), f.query())
        .await
        .unwrap()
        .0;
    assert!(live_panel.contains(&binding.as_uuid().to_string()));
    assert!(live_panel.contains("Revoke binding access now"));
    assert!(!live_panel.contains("unsaved &lt;/textarea&gt;"));
    for user in [f.f.member.id, f.f.stranger.id] {
        assert!(matches!(
            revoke_json(
                f.workspace(user),
                Path((f.f.company.id, f.agent.id)),
                Json(RevokeInput {
                    binding: Some(binding),
                    skill: None
                })
            )
            .await,
            Err(AppError::NotFound(_))
        ));
        assert!(matches!(
            preview_json(f.workspace(user), Path((f.f.company.id, f.agent.id))).await,
            Err(AppError::NotFound(_))
        ));
    }
    assert!(matches!(
        preview_json(
            f.workspace(f.f.stranger.id),
            Path((f.f.foreign.id, f.agent.id))
        )
        .await,
        Err(AppError::NotFound(_))
    ));
    revoke_json(
        f.workspace(f.f.owner.id),
        Path((f.f.company.id, f.agent.id)),
        Json(RevokeInput {
            binding: Some(binding),
            skill: None,
        }),
    )
    .await
    .unwrap();
    let state = f.state().await;
    let active = state.active.unwrap();
    assert_eq!(active.id, revision.id);
    assert_eq!(active.tools[0].access, AgentGrantState::Revoked);
    assert_eq!(active.tools[0].assignment.binding, Some(binding));
    let revoked_panel = panel(f.workspace(f.f.owner.id), Path(f.agent.id), f.query())
        .await
        .unwrap()
        .0;
    assert!(revoked_panel.contains("select it again, save and publish"));
}

#[tokio::test]
async fn autonomous_agent_authoring_skill_revocation_form_uses_exact_assignment_and_safe_panel() {
    let Some(f) = Fixture::new().await else {
        return;
    };
    let skill = f.skill("triage", Vec::new()).await;
    let mut selected = fields(f.agent.draft_revision, "[]");
    selected
        .iter_mut()
        .find(|(name, _)| *name == "skill_ids")
        .unwrap()
        .1 = skill.id.to_string();
    super::super::super::update_agent(
        f.workspace(f.f.owner.id),
        Path(f.agent.id),
        f.query(),
        Form(form(&selected).await),
    )
    .await
    .unwrap();
    let saved = f.state().await;
    let revision = f
        .workflow
        .agent_publications()
        .publish_agent(PublishAgent {
            scope: f.scope(),
            expected_draft_revision: saved.agent.draft_revision,
            expected_active_revision: None,
            key: AgentPublicationKey::parse(Uuid::new_v4().to_string()).unwrap(),
        })
        .await
        .unwrap();
    let active = f.state().await.active.unwrap();
    let assignment = active.skills[0].assignment;
    let live = panel(f.workspace(f.f.owner.id), Path(f.agent.id), f.query())
        .await
        .unwrap()
        .0;
    assert!(
        live.contains(&assignment.as_uuid().to_string())
            && live.contains("Revoke skill access now")
    );
    assert!(!live.contains("Private instructions for triage"));
    let revoked = revoke_form(
        f.workspace(f.f.owner.id),
        Path(f.agent.id),
        f.query(),
        Form(RevokeInput {
            binding: None,
            skill: Some(assignment),
        }),
    )
    .await
    .unwrap()
    .0;
    assert!(revoked.contains("Access revoked") && revoked.contains("new skill assignment"));
    let active = f.state().await.active.unwrap();
    assert_eq!(active.id, revision.id);
    assert_eq!(active.skills[0].assignment, assignment);
    assert_eq!(active.skills[0].access, AgentGrantState::Revoked);
}

#[tokio::test]
async fn autonomous_agent_authoring_skill_order_survives_classic_metadata_save_and_stale_rejection()
{
    use axum::extract::State;
    let Some(f) = Fixture::new().await else {
        return;
    };
    let alpha = f.skill("alpha", Vec::new()).await;
    let zeta = f.skill("zeta", Vec::new()).await;
    let mut ordered = fields(f.agent.draft_revision, "[]");
    ordered
        .iter_mut()
        .find(|(name, _)| *name == "skill_ids")
        .unwrap()
        .1 = format!("{},{}", zeta.id, alpha.id);
    super::super::super::update_agent(
        f.workspace(f.f.owner.id),
        Path(f.agent.id),
        f.query(),
        Form(form(&ordered).await),
    )
    .await
    .unwrap();
    let before = f.state().await;
    assert_eq!(
        before
            .skills
            .iter()
            .map(|skill| skill.id)
            .collect::<Vec<_>>(),
        [zeta.id, alpha.id]
    );
    let mut metadata = fields(before.agent.draft_revision, "[]");
    metadata.retain(|(name, _)| !matches!(*name, "skill_ids" | "tool_assignments"));
    let classic = |submitted| {
        crate::adapters::http::routes::agent::update_agent_handler(
            State(f.f.companies.clone()),
            State(f.f.agents.clone()),
            State(f.workspace(f.f.owner.id).skill_use_cases),
            AuthenticatedUser { id: f.f.owner.id },
            Path((f.f.company.id, f.agent.id)),
            Form(submitted),
        )
    };
    let classic_saved = classic(form(&metadata).await).await.unwrap().0;
    assert!(classic_saved.contains(&format!("agent-row-{}", f.agent.id)));
    let saved = f.state().await;
    assert_eq!(
        saved
            .skills
            .iter()
            .map(|skill| skill.id)
            .collect::<Vec<_>>(),
        [zeta.id, alpha.id]
    );
    assert_eq!(
        saved.agent.draft_revision.get(),
        before.agent.draft_revision.get() + 1
    );
    let stale = classic(form(&metadata).await).await.unwrap().0;
    assert!(
        stale.contains("Reload it before saving") && stale.contains("unsaved &lt;/textarea&gt;")
    );
    assert!(stale.contains(&format!(
        r#"name="expected_revision" value="{}""#,
        before.agent.draft_revision.get()
    )));
    assert_eq!(
        f.state().await.agent.draft_revision,
        saved.agent.draft_revision
    );
}

#[tokio::test]
async fn autonomous_agent_authoring_missing_skill_tool_fails_shared_preview_without_changing_active()
 {
    let Some(f) = Fixture::new().await else {
        return;
    };
    let published = f
        .workflow
        .agent_publications()
        .publish_agent(PublishAgent {
            scope: f.scope(),
            expected_draft_revision: f.agent.draft_revision,
            expected_active_revision: None,
            key: AgentPublicationKey::parse(Uuid::new_v4().to_string()).unwrap(),
        })
        .await
        .unwrap();
    let skill = f
        .skill(
            "requires-lookup",
            vec![crate::domain::workflow::TypeName::parse("customer.lookup").unwrap()],
        )
        .await;
    let mut selected = fields(f.agent.draft_revision, "[]");
    selected
        .iter_mut()
        .find(|(name, _)| *name == "skill_ids")
        .unwrap()
        .1 = skill.id.to_string();
    super::super::super::update_agent(
        f.workspace(f.f.owner.id),
        Path(f.agent.id),
        f.query(),
        Form(form(&selected).await),
    )
    .await
    .unwrap();
    let error = preview_json(
        f.workspace(f.f.owner.id),
        Path((f.f.company.id, f.agent.id)),
    )
    .await
    .err()
    .unwrap();
    assert!(matches!(error, AppError::BadRequest(_)));
    assert!(error.to_string().contains("customer.lookup"), "{error}");
    assert!(!error.to_string().contains("Private instructions"));
    let state = f.state().await;
    assert_eq!(state.active.unwrap().id, published.id);
    assert_eq!(state.history.items.len(), 1);
}

#[tokio::test]
async fn autonomous_agent_authoring_malformed_skill_csv_keeps_input_and_does_not_clear_assignments()
{
    let Some(f) = Fixture::new().await else {
        return;
    };
    let mut submitted = fields(f.agent.draft_revision, "[]");
    submitted
        .iter_mut()
        .find(|(name, _)| *name == "skill_ids")
        .unwrap()
        .1 = "invalid-uuid,<hostile>".into();
    let response = super::super::super::update_agent(
        f.workspace(f.f.owner.id),
        Path(f.agent.id),
        f.query(),
        Form(form(&submitted).await),
    )
    .await
    .unwrap();
    let html = text(response).await;
    assert!(html.contains(r#"name="skill_ids" value="invalid-uuid,&lt;hostile&gt;""#));
    assert!(html.contains("unsaved &lt;/textarea&gt;"));
    assert_eq!(f.state().await.agent.draft_revision, f.agent.draft_revision);
}
