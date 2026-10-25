//! The bindings tab and the channel card: a keyed create that a double submit cannot repeat,
//! schema-driven params, refusals that keep every choice, a version change that re-renders the
//! form, configure and activation at the revision seen, channels an admin cannot see, other
//! tenants and plain members, and hostile names.
use super::*;
use crate::application::workflow::reads::BindingUse;

/// A source whose parameters the form shows as fields.
fn with_params(workflow: Uuid, extra: serde_json::Value) -> String {
    let mut source = example("data.map", workflow);
    let mut properties = json!({
        "reviewer": {"type": "string", "maxLength": 64, "description": "Who reviews"},
        "tone": {"enum": ["warm", "brief"]},
        "limit": {"type": "integer"}
    });
    for (name, schema) in extra.as_object().unwrap() {
        properties[name] = schema.clone();
    }
    source["parameter_schema"] = json!({
        "type": "object",
        "properties": properties,
        "required": ["reviewer"],
        "additionalProperties": false
    });
    serde_json::to_string_pretty(&source).unwrap()
}

impl Fixture {
    async fn save_binding(&self, user: Uuid, workflow: Uuid, fields: &[(&str, &str)]) -> String {
        save_binding(self.ui(user), Path(workflow), self.query(), form(fields))
            .await
            .unwrap()
            .0
    }

    async fn activity(
        &self,
        user: Uuid,
        workflow: Uuid,
        binding: Uuid,
        revision: u64,
        active: bool,
    ) -> WorkflowResult<String> {
        let form = axum::Form(ActivityForm {
            expected_state_revision: revision,
        });
        let path = Path((workflow, binding));
        if active {
            activate_binding(self.ui(user), path, self.query(), form).await
        } else {
            deactivate_binding(self.ui(user), path, self.query(), form).await
        }
        .map(|html| html.0)
    }

    async fn usage(&self, workflow: Uuid) -> Vec<BindingUse> {
        self.engine
            .authoring_reads()
            .workflow_usage(CompanyId::new(self.company), WorkflowId::new(workflow))
            .await
            .unwrap()
            .bindings
    }
}

#[tokio::test]
async fn a_binding_is_created_once_paused_then_activated_at_the_revision_seen() {
    let Some(f) = Fixture::new().await else {
        return;
    };
    let workflow = Uuid::new_v4();
    let version = f
        .published(workflow, "Support", &with_params(workflow, json!({})))
        .await
        .to_string();
    let pane = f.pane(f.owner, workflow, "bindings").await.unwrap();
    assert!(
        pane.contains("Not bound: nothing runs this workflow"),
        "{pane}"
    );
    assert!(pane.contains(r#"name="param:reviewer""#) && pane.contains(r#"maxlength="64""#));
    assert!(pane.contains("reviewer (required)") && pane.contains("Who reviews"));
    assert!(pane.contains(r#"<option value="warm">warm</option>"#));
    // The owner names the run-as; the fields carry the form's key.
    assert!(pane.contains(r#"name="run_as""#));
    let key = field_value(&pane, "form_key");
    let fields = [
        ("form_key", key.as_str()),
        ("version_id", version.as_str()),
        ("association", "company"),
        ("param:reviewer", "rev-1"),
        ("param:tone", "warm"),
        ("param:limit", ""),
        ("message_trigger", ""),
    ];
    let saved = f.save_binding(f.owner, workflow, &fields).await;
    assert!(saved.contains("Saved binding"), "{saved}");
    assert!(saved.contains("paused until you activate it"));
    assert!(saved.contains("paused: admits nothing") && !saved.contains("Not bound"));
    let usage = f.usage(workflow).await;
    assert_eq!(usage.len(), 1);
    assert_eq!(
        usage[0].binding.params,
        json!({"reviewer": "rev-1", "tone": "warm"})
    );
    assert_eq!(usage[0].binding.run_as, f.owner);
    // A double submit of the same form creates nothing more.
    let again = f.save_binding(f.owner, workflow, &fields).await;
    assert!(again.contains("was already created"), "{again}");
    assert_eq!(f.usage(workflow).await.len(), 1);

    let binding = usage[0].binding.binding.as_uuid();
    let revision = usage[0].binding.state_revision;
    let active = f
        .activity(f.owner, workflow, binding, revision, true)
        .await
        .unwrap();
    assert!(active.contains("Activated binding"), "{active}");
    assert!(active.contains(">Deactivate</button>"));
    // The page's revision is stale now: nothing changes, and the row says why.
    let stale = f
        .activity(f.owner, workflow, binding, revision, false)
        .await
        .unwrap();
    assert!(
        stale.contains("This binding changed since the page was shown"),
        "{stale}"
    );
    assert!(f.usage(workflow).await[0].binding.active);
    let paused = f
        .activity(f.owner, workflow, binding, revision + 1, false)
        .await
        .unwrap();
    assert!(paused.contains("Deactivated binding"), "{paused}");
}

#[tokio::test]
async fn a_refused_binding_keeps_every_choice_and_says_why() {
    let Some(f) = Fixture::new().await else {
        return;
    };
    let workflow = Uuid::new_v4();
    let version = f
        .published(workflow, "Support", &with_params(workflow, json!({})))
        .await
        .to_string();
    let base = |limit: &'static str, reviewer: &'static str, trigger: &'static str| {
        let version = version.clone();
        move |key: &str| {
            vec![
                ("form_key".to_owned(), key.to_owned()),
                ("version_id".to_owned(), version.clone()),
                ("association".to_owned(), "company".to_owned()),
                ("param:reviewer".to_owned(), reviewer.to_owned()),
                ("param:tone".to_owned(), "brief".to_owned()),
                ("param:limit".to_owned(), limit.to_owned()),
                ("message_trigger".to_owned(), trigger.to_owned()),
            ]
        }
    };
    let submit = async |fields: Vec<(String, String)>| {
        let pairs: Vec<(&str, &str)> = fields
            .iter()
            .map(|(name, value)| (name.as_str(), value.as_str()))
            .collect();
        f.save_binding(f.owner, workflow, &pairs).await
    };
    // A field that does not parse as its type: nothing is sent to the command.
    let html = submit(base("7.5", "rev-1", "")("k1")).await;
    assert!(
        html.contains("Parameter limit must be a whole number."),
        "{html}"
    );
    assert!(html.contains(r#"value="rev-1""#) && html.contains(r#"value="7.5""#));
    assert!(html.contains(r#"<option value="brief" selected>"#));
    // The command's own refusals: a missing required parameter, a trigger on the company.
    let html = submit(base("3", "", "")("k2")).await;
    assert!(html.contains("Not saved:"), "{html}");
    assert!(html.contains(r#"value="3""#));
    let html = submit(base("3", "rev-1", "direct")("k3")).await;
    assert!(html.contains("Not saved:"), "{html}");
    assert!(html.contains(r#"<option value="direct" selected>"#));
    assert!(f.usage(workflow).await.is_empty());
}

#[tokio::test]
async fn a_version_change_renders_that_versions_fields_and_keeps_matching_values() {
    let Some(f) = Fixture::new().await else {
        return;
    };
    let workflow = Uuid::new_v4();
    f.published(workflow, "Support", &with_params(workflow, json!({})))
        .await;
    let newer = f
        .published(
            workflow,
            "Support",
            &with_params(workflow, json!({"loud": {"type": "boolean"}})),
        )
        .await;
    let query = format!(
        "company_id={}&version_id={newer}&association=company&param%3Areviewer=kept&param%3Atone=warm",
        f.company
    );
    let html = binding_form(
        f.ui(f.owner),
        Path(workflow),
        Query(BindingFormQuery {
            company_id: f.company,
            binding_id: None,
        }),
        format!("/ui/workflows/{workflow}/bindings/form?{query}")
            .parse()
            .unwrap(),
    )
    .await
    .unwrap()
    .0;
    assert!(html.starts_with(r#"<section id="binding-form""#), "{html}");
    assert!(html.contains(&format!(r#"<option value="{newer}" selected>"#)));
    assert!(html.contains(r#"name="param:loud""#) && html.contains(r#"value="kept""#));
    assert!(html.contains(r#"<option value="warm" selected>"#));
}

#[tokio::test]
async fn configure_saves_at_the_revision_seen_and_a_stale_one_keeps_the_choices() {
    let Some(f) = Fixture::new().await else {
        return;
    };
    let workflow = Uuid::new_v4();
    let version = f
        .published(workflow, "Support", &with_params(workflow, json!({})))
        .await
        .to_string();
    let pane = f.pane(f.owner, workflow, "bindings").await.unwrap();
    let key = field_value(&pane, "form_key");
    f.save_binding(
        f.owner,
        workflow,
        &[
            ("form_key", &key),
            ("version_id", &version),
            ("association", "company"),
            ("param:reviewer", "rev-1"),
        ],
    )
    .await;
    let binding = f.usage(workflow).await[0].binding.binding.as_uuid();
    let opened = binding_form(
        f.ui(f.owner),
        Path(workflow),
        Query(BindingFormQuery {
            company_id: f.company,
            binding_id: Some(binding),
        }),
        "/".parse().unwrap(),
    )
    .await
    .unwrap()
    .0;
    assert!(opened.contains("Configure binding"), "{opened}");
    assert!(opened.contains("Fixed once created"));
    assert_eq!(field_value(&opened, "expected_state_revision"), "1");
    assert!(opened.contains(r#"value="rev-1""#));
    let id = binding.to_string();
    let f = &f;
    let edit = |reviewer: &'static str, revision: &'static str| {
        let (id, version) = (id.clone(), version.clone());
        async move {
            f.save_binding(
                f.owner,
                workflow,
                &[
                    ("binding_id", &id),
                    ("expected_state_revision", revision),
                    ("version_id", &version),
                    ("param:reviewer", reviewer),
                ],
            )
            .await
        }
    };
    let saved = edit("rev-2", "1").await;
    assert!(saved.contains("Saved binding"), "{saved}");
    assert_eq!(
        f.usage(workflow).await[0].binding.params["reviewer"],
        "rev-2"
    );
    let stale = edit("rev-3", "1").await;
    assert!(stale.contains("Someone changed this binding"), "{stale}");
    assert!(stale.contains(r#"value="rev-3""#));
    assert_eq!(
        f.usage(workflow).await[0].binding.params["reviewer"],
        "rev-2"
    );
}

#[tokio::test]
async fn channels_an_admin_cannot_see_stay_out_of_the_tab_the_form_and_the_card() {
    let Some(f) = Fixture::new().await else {
        return;
    };
    let workflow = Uuid::new_v4();
    let version = f
        .published(workflow, "Support", &with_params(workflow, json!({})))
        .await
        .to_string();
    let board = f.channel("Board", "board", true).await;
    let open = f.channel("Open desk", "open-desk", false).await;
    let bind = async |user: Uuid, channel: Uuid, trigger: &str| {
        let pane = f.pane(user, workflow, "bindings").await.unwrap();
        let key = field_value(&pane, "form_key");
        f.save_binding(
            user,
            workflow,
            &[
                ("form_key", &key),
                ("version_id", &version),
                ("association", &format!("channel:{channel}")),
                ("param:reviewer", "rev-1"),
                ("message_trigger", trigger),
            ],
        )
        .await
    };
    let saved = bind(f.owner, board, "").await;
    assert!(
        saved.contains("Saved binding") && saved.contains("Channel Board"),
        "{saved}"
    );
    // The admin manages the workflow but is not on the board's allowlist.
    let admin = f.pane(f.admin, workflow, "bindings").await.unwrap();
    assert!(!admin.contains("Board"), "{admin}");
    assert!(admin.contains("1 more of the listed binding is on channels you cannot see"));
    assert!(admin.contains(&format!(
        r#"<option value="channel:{open}">Channel Open desk</option>"#
    )));
    let refused = bind(f.admin, board, "").await;
    assert!(refused.contains("Not saved: A chosen channel"), "{refused}");
    let card = async |user: Uuid, channel: Uuid| {
        channel_bindings(f.ui(user), Path(channel), f.query())
            .await
            .map(|html| html.0)
    };
    assert!(matches!(
        card(f.admin, board).await,
        Err(AppError::NotFound(_))
    ));
    let quiet = card(f.admin, open).await.unwrap();
    assert!(
        quiet.contains("No active workflow answers this channel's messages"),
        "{quiet}"
    );
    assert!(quiet.contains("No workflow is bound to this channel."));
    // An active binding with a message trigger answers the channel.
    let saved = bind(f.owner, open, "direct").await;
    assert!(saved.contains("Saved binding"), "{saved}");
    let on_open = f
        .usage(workflow)
        .await
        .into_iter()
        .find(|binding| binding.channel_name.as_deref() == Some("Open desk"))
        .unwrap();
    let active = f
        .activity(
            f.owner,
            workflow,
            on_open.binding.binding.as_uuid(),
            on_open.binding.state_revision,
            true,
        )
        .await
        .unwrap();
    assert!(active.contains("Activated binding"), "{active}");
    let answering = card(f.admin, open).await.unwrap();
    assert!(
        answering.contains("An active workflow answers"),
        "{answering}"
    );
    assert!(answering.contains("message trigger direct") && answering.contains("Support"));
}

#[tokio::test]
async fn other_tenants_and_plain_members_reach_no_binding() {
    let Some(f) = Fixture::new().await else {
        return;
    };
    let workflow = Uuid::new_v4();
    let version = f
        .published(workflow, "Support", &with_params(workflow, json!({})))
        .await
        .to_string();
    let pane = f.pane(f.owner, workflow, "bindings").await.unwrap();
    let key = field_value(&pane, "form_key");
    f.save_binding(
        f.owner,
        workflow,
        &[
            ("form_key", &key),
            ("version_id", &version),
            ("association", "company"),
            ("param:reviewer", "rev-1"),
        ],
    )
    .await;
    let binding = f.usage(workflow).await[0].binding.binding.as_uuid();
    let not_found =
        |error: &WorkflowError| matches!(error, WorkflowError::App(AppError::NotFound(_)));
    // A member manages nothing; a stranger names its own company with this workflow's ids.
    for (user, company) in [(f.member, f.company), (f.stranger, f.other)] {
        let query = || {
            Query(CompanyQuery {
                company_id: company,
            })
        };
        let saved = save_binding(
            f.ui(user),
            Path(workflow),
            query(),
            form(&[
                ("binding_id", &binding.to_string()),
                ("expected_state_revision", "1"),
                ("version_id", &version),
            ]),
        )
        .await;
        assert!(not_found(&saved.unwrap_err()));
        let activated = activate_binding(
            f.ui(user),
            Path((workflow, binding)),
            query(),
            axum::Form(ActivityForm {
                expected_state_revision: 1,
            }),
        )
        .await;
        assert!(not_found(&activated.unwrap_err()));
        let opened = binding_form(
            f.ui(user),
            Path(workflow),
            Query(BindingFormQuery {
                company_id: company,
                binding_id: Some(binding),
            }),
            "/".parse().unwrap(),
        )
        .await;
        assert!(matches!(opened, Err(AppError::NotFound(_))));
    }
    assert!(!f.usage(workflow).await[0].binding.active);
}

#[tokio::test]
async fn hostile_names_are_escaped_in_the_tab_the_form_and_the_card() {
    let Some(f) = Fixture::new().await else {
        return;
    };
    let hostile = format!("<{}>alert(1)</script>", "script");
    let escaped = "&lt;script&gt;alert(1)&lt;/script&gt;";
    let workflow = Uuid::new_v4();
    let mut source = example("data.map", workflow);
    source["parameter_schema"] = json!({
        "type": "object",
        "properties": {"note": {"type": "string", "description": hostile}}
    });
    let version = f
        .published(
            workflow,
            &hostile,
            &serde_json::to_string_pretty(&source).unwrap(),
        )
        .await
        .to_string();
    let channel = f.channel(&hostile, "hostile", false).await;
    sqlx::query("UPDATE users SET username = $2 WHERE id = $1")
        .bind(f.admin)
        .bind(&hostile)
        .execute(f.persistence.pool())
        .await
        .unwrap();
    let pane = f.pane(f.owner, workflow, "bindings").await.unwrap();
    let key = field_value(&pane, "form_key");
    let saved = f
        .save_binding(
            f.owner,
            workflow,
            &[
                ("form_key", &key),
                ("version_id", &version),
                ("association", &format!("channel:{channel}")),
                ("param:note", &hostile),
            ],
        )
        .await;
    let card = channel_bindings(f.ui(f.owner), Path(channel), f.query())
        .await
        .unwrap()
        .0;
    for html in [&pane, &saved, &card] {
        assert!(!html.contains(&hostile), "{html}");
        assert!(html.contains(escaped), "{html}");
    }
}

#[test]
fn an_archived_workflows_binding_can_still_be_paused_but_not_activated_or_configured() {
    use crate::application::workflow::reads::BindingSummary;
    use crate::domain::workflow::{VersionId, WorkflowBindingId};
    let (company, workflow) = (Uuid::new_v4(), Uuid::new_v4());
    let binding = |active: bool| BindingUse {
        binding: BindingSummary {
            binding: WorkflowBindingId::new(Uuid::new_v4()),
            state_revision: 4,
            configuration_revision: 1,
            active,
            association: crate::application::workflow::RelatedAssociation::Company,
            workflow: WorkflowId::new(workflow),
            version: VersionId::new(Uuid::new_v4()),
            params: json!({}),
            run_as: Uuid::new_v4(),
            message_trigger: None,
        },
        workflow_title: "Support".into(),
        channel_name: None,
        resources: BTreeMap::new(),
        schedules: Vec::new(),
        schedules_truncated: false,
    };
    let render = |bindings: &[BindingUse]| {
        pages::workflow_bindings_body(&pages::BindingsView {
            company_id: company,
            workflow,
            archived: true,
            versions: &[],
            bindings,
            truncated: false,
            hidden: 0,
            people: &[],
            banner: None,
            form_html: "",
        })
    };
    let active = render(&[binding(true)]);
    assert!(
        active.contains("/deactivate?") && active.contains(r#""expected_state_revision": "4""#)
    );
    assert!(
        !active.contains("/activate?") && !active.contains("Configure"),
        "{active}"
    );
    let paused = render(&[binding(false)]);
    assert!(!paused.contains("<button"), "{paused}");
}
