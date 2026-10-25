//! The editor's writes — save, validate, publish, archive — and the editor tab it renders.
use super::*;

impl Ui {
    pub(super) async fn editor_body(
        &self,
        company: Uuid,
        detail: &DefinitionDetail,
        state: EditorState,
    ) -> AppResult<String> {
        let workflow = detail.summary.workflow.as_uuid();
        let draft = match &state.typed {
            Some(typed) => pages::EditorDraft {
                title: &typed.title,
                description: &typed.description,
                source: &typed.source,
                expected_revision: typed.expected_revision,
            },
            None => pages::EditorDraft {
                title: &detail.summary.title,
                description: &detail.description,
                source: &detail.source,
                expected_revision: detail.summary.revision,
            },
        };
        let needs = WorkflowSourceDecoder
            .decode(draft.source)
            .and_then(|decoded| publication_needs(&decoded));
        let (options, too_many_tools) = match &needs {
            Ok(needs) => self.editor_options(company, needs).await?,
            Err(_) => (PublicationOptions::default(), false),
        };
        let unsaved = state.typed.as_ref().is_some_and(|typed| {
            typed.title != detail.summary.title
                || typed.description != detail.description
                || typed.source != detail.source
        });
        let selections = match (&state.selections, &needs) {
            (Some(selections), _) => selections.clone(),
            (None, Ok(needs)) => default_selections(needs, &options),
            (None, Err(_)) => pages::WorkflowSelections::default(),
        };
        let preview_html = match &state.preview {
            None => None,
            Some((revision, bundle)) => {
                let agent_names: BTreeMap<Uuid, String> = self
                    .agents
                    .list_company_agents(self.user, company)
                    .await?
                    .into_iter()
                    .map(|agent| (agent.id, agent.name))
                    .collect();
                Some(pages::workflow_preview(&pages::PreviewView {
                    company_id: company,
                    workflow,
                    revision: *revision,
                    bundle,
                    selections: &selections,
                    agent_names: &agent_names,
                    options: &options,
                }))
            }
        };
        let banner = state.banner.as_ref().map(|banner| match banner {
            Banner::Saved(message) => pages::EditorBanner::Saved(message),
            Banner::Conflict { current, theirs } => pages::EditorBanner::Conflict {
                current: *current,
                theirs,
            },
            Banner::MovedAfterSave {
                saved,
                current,
                theirs,
            } => pages::EditorBanner::MovedAfterSave {
                saved: *saved,
                current: *current,
                theirs,
            },
            Banner::Refused(message) => pages::EditorBanner::Refused(message),
        });
        Ok(pages::workflow_editor(&pages::EditorView {
            company_id: company,
            workflow,
            archived: detail.summary.archived,
            draft,
            banner,
            diagnostic: state.diagnostic.as_ref(),
            preview_html: preview_html.as_deref(),
            needs: needs.as_ref(),
            options: &options,
            selections: &selections,
            unsaved,
            too_many_tools,
        }))
    }

    /// The connections offering each tool the draft names, for at most `MAX_OPTION_NAMES` names
    /// of a kind; a draft naming more still renders, with the second value saying so (F013).
    pub(super) async fn editor_options(
        &self,
        company: Uuid,
        needs: &PublicationNeeds,
    ) -> AppResult<(PublicationOptions, bool)> {
        if needs.is_empty() {
            return Ok((PublicationOptions::default(), false));
        }
        let names = |tools: Vec<String>| {
            let mut names: Vec<String> = tools
                .into_iter()
                .collect::<std::collections::BTreeSet<_>>()
                .into_iter()
                .collect();
            let cut = names.len() > MAX_OPTION_NAMES;
            names.truncate(MAX_OPTION_NAMES);
            (names, cut)
        };
        let (mcp, mcp_cut) = names(needs.mcp.iter().map(|n| n.tool.as_str().into()).collect());
        let (http, http_cut) = names(needs.tools.iter().map(|n| n.tool.as_str().into()).collect());
        let options = self
            .engine
            .authoring_reads()
            .publication_options(super::company(company), &mcp, &http)
            .await?;
        Ok((options, mcp_cut || http_cut))
    }
}

/// POST /ui/workflows/{workflow}/save — save against the revision the form was opened with,
/// then (intent `validate`) compile that revision. A refused save keeps the typed text; a
/// conflict shows what the newer revision has.
pub(super) async fn save(
    ui: Ui,
    Path(workflow): Path<Uuid>,
    Query(query): Query<CompanyQuery>,
    body: Bytes,
) -> WorkflowResult<Html<String>> {
    let company = ui.company(query.company_id).await?.id;
    let form = FormFields::parse(&body);
    let selections = pages::WorkflowSelections::from_fields(form.pairs());
    let typed = Typed {
        title: form.text("title"),
        description: form.text("description"),
        source: form.text("source"),
        expected_revision: form.number("expected_revision")?,
    };
    let saved = save_draft(
        &ui.engine,
        company,
        ui.actor()?,
        workflow,
        Some(typed.expected_revision),
        DraftContent {
            title: typed.title.clone(),
            description: typed.description.clone(),
            source: typed.source.clone(),
        },
    )
    .await;
    let state = match saved {
        Ok(draft) => {
            let revision = draft.revision().get();
            if form.get("intent") == Some("validate") {
                validated(&ui, company, workflow, revision, typed, selections).await?
            } else {
                EditorState {
                    banner: Some(Banner::Saved(format!("Saved revision {revision}."))),
                    selections: Some(selections),
                    ..Default::default()
                }
            }
        }
        Err(WorkflowError::App(AppError::Conflict(_))) => {
            let current = ui.detail(company, workflow).await?;
            let banner = if current.summary.archived {
                Banner::Refused(
                    "This workflow was archived; the draft can no longer be saved.".into(),
                )
            } else {
                Banner::Conflict {
                    current: current.summary.revision,
                    theirs: current.source.clone(),
                }
            };
            EditorState {
                typed: Some(Typed {
                    // The next save deliberately replaces the newer revision.
                    expected_revision: current.summary.revision,
                    ..typed
                }),
                banner: Some(banner),
                selections: Some(selections),
                ..Default::default()
            }
        }
        Err(WorkflowError::App(AppError::BadRequest(message))) => EditorState {
            typed: Some(typed),
            banner: Some(Banner::Refused(message)),
            selections: Some(selections),
            ..Default::default()
        },
        Err(error) => return Err(error),
    };
    let pane = ui
        .pane_html(company, workflow, pages::WorkflowTab::Editor, state)
        .await?;
    // The sidebar shows the title and revision a save may have changed (F009).
    let menu = ui.menu_oob(company, workflow).await?;
    Ok(Html(format!("{pane}{menu}")))
}

/// Compile the saved revision with the selections, as the editor reports it. A draft that moved
/// again before validation keeps the author's text against the newer revision (F008).
pub(super) async fn validated(
    ui: &Ui,
    company: Uuid,
    workflow: Uuid,
    revision: u64,
    typed: Typed,
    selections: pages::WorkflowSelections,
) -> WorkflowResult<EditorState> {
    let (http, mcp) = selection_bodies(&selections);
    let result = validate_draft(
        &ui.engine,
        company,
        ui.actor()?,
        workflow,
        revision,
        &http,
        &mcp,
    )
    .await;
    let mut state = match result {
        Ok(bundle) => EditorState {
            banner: Some(Banner::Saved(format!(
                "Saved revision {revision}; it is valid."
            ))),
            preview: Some((revision, bundle)),
            ..Default::default()
        },
        Err(WorkflowError::App(AppError::Conflict(_))) => {
            let current = ui.detail(company, workflow).await?;
            EditorState {
                banner: Some(Banner::MovedAfterSave {
                    saved: revision,
                    current: current.summary.revision,
                    theirs: current.source.clone(),
                }),
                typed: Some(Typed {
                    expected_revision: current.summary.revision,
                    ..typed
                }),
                ..Default::default()
            }
        }
        Err(error) => {
            let mut state = refusal(error)?;
            state.banner = Some(match state.banner.take() {
                // The save went through; say so beside why validation did not.
                Some(Banner::Refused(why)) => Banner::Refused(format!(
                    "Saved revision {revision}, but it could not be validated: {why}"
                )),
                _ => Banner::Saved(format!(
                    "Saved revision {revision}; it does not validate yet."
                )),
            });
            state
        }
    };
    state.selections = Some(selections);
    Ok(state)
}

/// POST /ui/workflows/{workflow}/publish — publish the validated revision with the validated
/// selections, then show where every version runs.
pub(super) async fn publish(
    ui: Ui,
    Path(workflow): Path<Uuid>,
    Query(query): Query<CompanyQuery>,
    body: Bytes,
) -> WorkflowResult<Response> {
    let company = ui.company(query.company_id).await?.id;
    let form = FormFields::parse(&body);
    let selections = pages::WorkflowSelections::from_fields(form.pairs());
    let (http, mcp) = selection_bodies(&selections);
    let expected = form.number("expected_revision")?;
    let published = publish_draft(
        &ui.engine,
        company,
        ui.actor()?,
        workflow,
        PublishRequest {
            expected_revision: expected,
            command_key: &form.text("command_key"),
            http: &http,
            mcp: &mcp,
        },
    )
    .await;
    match published {
        Ok((version, bundle)) => {
            let detail = ui.detail(company, workflow).await?;
            // Publication re-captures agents, skills and tools; say so when what it froze is
            // not what the preview showed (F010).
            let changed = form
                .get("previewed_dependencies")
                .is_some_and(|digest| digest != pages::dependency_digest(&bundle));
            let banner = format!(
                "Published version {} from draft revision {expected}. Publishing changes no binding: each keeps its version until you reconfigure it.{}",
                pages::short_id(version.as_uuid()),
                if changed {
                    " Its content differs from the preview you validated: an agent, skill, tool or approval changed in between. Review it before binding it."
                } else {
                    ""
                }
            );
            let body = ui
                .versions_body(company, &detail, Some(&banner), None)
                .await?;
            let pane = ui
                .render_pane(company, &detail, pages::WorkflowTab::Versions, body)
                .await;
            Ok(pushed(pane, &workflow_url(company, workflow, "versions")))
        }
        Err(WorkflowError::App(AppError::Conflict(message))) => {
            let current = ui.detail(company, workflow).await?;
            // Name the draft's move only when it moved; any other conflict is the command's
            // own (an agent that changed during publication, a key used for another version).
            let why = if current.summary.archived {
                format!("Revision {expected} was not published: the workflow is archived.")
            } else if current.summary.revision != expected {
                format!(
                    "Revision {expected} is no longer the draft (now revision {}). Validate the \
                     current draft again before publishing.",
                    current.summary.revision
                )
            } else {
                format!("Revision {expected} was not published: {message}")
            };
            let state = EditorState {
                banner: Some(Banner::Refused(why)),
                selections: Some(selections),
                ..Default::default()
            };
            Ok(Html(
                ui.pane_html(company, workflow, pages::WorkflowTab::Editor, state)
                    .await?,
            )
            .into_response())
        }
        Err(error) => {
            let mut state = refusal(error)?;
            state.selections = Some(selections);
            Ok(Html(
                ui.pane_html(company, workflow, pages::WorkflowTab::Editor, state)
                    .await?,
            )
            .into_response())
        }
    }
}

/// POST /ui/workflows/{workflow}/archive — archive the draft at the revision the page showed.
pub(super) async fn archive(
    ui: Ui,
    Path(workflow): Path<Uuid>,
    Query(query): Query<CompanyQuery>,
    body: Bytes,
) -> WorkflowResult<Response> {
    let company = ui.company(query.company_id).await?.id;
    let form = FormFields::parse(&body);
    let expected = form.number("expected_revision")?;
    let state = match archive_draft(&ui.engine, company, ui.actor()?, workflow, expected).await {
        Ok(()) => EditorState {
            banner: Some(Banner::Saved(
                "Archived. Its versions stay readable and keep serving the bindings that use them."
                    .into(),
            )),
            ..Default::default()
        },
        Err(WorkflowError::App(AppError::Conflict(_))) => EditorState {
            banner: Some(Banner::Refused(format!(
                "The draft changed since revision {expected}, or is already archived; nothing was archived."
            ))),
            ..Default::default()
        },
        Err(error) => return Err(error),
    };
    let pane = ui
        .pane_html(company, workflow, pages::WorkflowTab::Editor, state)
        .await?;
    let menu = ui.menu_oob(company, workflow).await?;
    Ok(Html(format!("{pane}{menu}")).into_response())
}
