//! The editor: located validation that keeps the typed source, concurrent saves and a draft that
//! moves after the save, the unsaved-text guard, archive, the sidebar refresh, and the MCP tools
//! the editor offers.
use super::*;

#[tokio::test]
async fn an_invalid_source_is_saved_kept_and_located() {
    let Some(f) = Fixture::new().await else {
        return;
    };
    let workflow = Uuid::new_v4();
    f.draft(workflow, "Broken", &blank_source(workflow)).await;
    // Line 13 names a step that does not exist.
    let broken =
        blank_source(workflow).replace("routes: {success: $end}", "routes: {success: nowhere}");
    let line = broken
        .lines()
        .position(|line| line.contains("nowhere"))
        .unwrap()
        + 1;
    let html = f
        .save(
            workflow,
            &[
                ("expected_revision", "1"),
                ("title", "Broken"),
                ("source", &broken),
                ("intent", "validate"),
            ],
        )
        .await
        .unwrap();
    assert!(
        html.contains("Saved revision 2; it does not validate yet."),
        "{html}"
    );
    assert!(html.contains(r#"id="workflow-diagnostic""#));
    assert!(html.contains(&format!("Go to line {line}")), "{html}");
    assert!(
        html.contains("routes: {success: nowhere}"),
        "the saved text is in the form"
    );
    assert!(!html.contains("Revision 2 is valid"));
    let stored = f
        .engine
        .reads()
        .definition(CompanyId::new(f.company), WorkflowId::new(workflow))
        .await
        .unwrap()
        .unwrap();
    assert_eq!(stored.summary.revision, 2);
    assert_eq!(stored.source, broken);
}

#[tokio::test]
async fn a_concurrent_save_keeps_the_authors_text_and_shows_what_the_other_saved() {
    let Some(f) = Fixture::new().await else {
        return;
    };
    let workflow = Uuid::new_v4();
    let source = blank_source(workflow);
    f.draft(workflow, "Shared", &source).await;
    // Someone else saves revision 2 while the author still has revision 1 open.
    let theirs = source.replace("max_steps: 10", "max_steps: 11");
    save_draft(
        &f.engine,
        f.company,
        WorkflowActor::authenticated(f.admin).unwrap(),
        workflow,
        Some(1),
        DraftContent {
            title: "Shared".into(),
            description: String::new(),
            source: theirs,
        },
    )
    .await
    .unwrap();
    let mine = source.replace("max_steps: 10", "max_steps: 12");
    let html = f
        .save(
            workflow,
            &[
                ("expected_revision", "1"),
                ("title", "Mine"),
                ("source", &mine),
                ("intent", "save"),
            ],
        )
        .await
        .unwrap();
    assert!(html.contains("Someone else saved revision 2"), "{html}");
    assert!(
        html.contains("max_steps: 12"),
        "the author's text stays in the form"
    );
    // The echoed text is unsaved: leaving the pane must ask (F003).
    assert!(html.contains("data-workflow-editor data-dirty"));
    assert!(
        html.contains("- limits: {max_steps: 11"),
        "the diff shows theirs"
    );
    assert!(
        html.contains("+ limits: {max_steps: 12"),
        "and the author's"
    );
    assert_eq!(field_value(&html, "expected_revision"), "2");
    let stored = f
        .engine
        .reads()
        .definition(CompanyId::new(f.company), WorkflowId::new(workflow))
        .await
        .unwrap()
        .unwrap();
    assert_eq!(
        stored.summary.revision, 2,
        "nothing of the author's was saved"
    );
    // Saving again deliberately replaces revision 2.
    let again = f
        .save(
            workflow,
            &[
                ("expected_revision", "2"),
                ("title", "Mine"),
                ("source", &mine),
                ("intent", "save"),
            ],
        )
        .await
        .unwrap();
    assert!(again.contains("Saved revision 3."), "{again}");
    assert!(!again.contains("data-dirty"), "a saved form starts clean");
}

#[tokio::test]
async fn archiving_refuses_a_stale_revision_then_locks_the_draft() {
    let Some(f) = Fixture::new().await else {
        return;
    };
    let workflow = Uuid::new_v4();
    f.draft(workflow, "Old", &blank_source(workflow)).await;
    let stale = text(
        archive(
            f.ui(f.owner),
            Path(workflow),
            f.query(),
            form(&[("expected_revision", "7")]),
        )
        .await
        .unwrap(),
    )
    .await;
    assert!(stale.contains("nothing was archived"), "{stale}");
    let done = text(
        archive(
            f.ui(f.owner),
            Path(workflow),
            f.query(),
            form(&[("expected_revision", "1")]),
        )
        .await
        .unwrap(),
    )
    .await;
    assert!(done.contains("Archived."), "{done}");
    assert!(done.contains(r#"<span class="badge badge-ghost">archived</span>"#));
    assert!(done.contains("This workflow is archived"));
    let refused = f
        .save(
            workflow,
            &[
                ("expected_revision", "2"),
                ("title", "Old"),
                ("source", &blank_source(workflow)),
                ("intent", "save"),
            ],
        )
        .await
        .unwrap();
    assert!(refused.contains("This workflow was archived"), "{refused}");
}

#[tokio::test]
async fn the_editor_offers_a_needed_mcp_tool_past_any_company_wide_count_and_cuts_per_tool() {
    let Some(f) = Fixture::new().await else {
        return;
    };
    // 201 other approved tools sort before the needed `lookup` (WF-P09-F002).
    let mut tools: Vec<String> = (0..201).map(|index| format!("a{index:03}")).collect();
    tools.push("lookup".into());
    let first = approved_rows(&f, "server-000", &tools).await;
    let workflow = Uuid::new_v4();
    f.draft(workflow, "Lookup", &source_of("mcp.call", workflow))
        .await;
    let html = f.pane(f.owner, workflow, "editor").await.unwrap();
    assert!(html.contains(r#"name="mcp:service:lookup""#), "{html}");
    assert!(!html.contains("no company MCP connection has this tool approved"));
    assert!(
        html.contains(&format!(r#"<option value="{first}" selected>"#)),
        "the only candidate"
    );
    assert!(
        !html.contains("a200"),
        "only the needed tool's approvals are read"
    );
    assert!(!html.contains("super-secret-token") && !html.contains("mcp.example.test"));
    // Past the per-tool bound the list says it was cut.
    for index in 1..=crate::application::workflow::reads::MAX_CONNECTIONS_PER_TOOL {
        approved_rows(&f, &format!("server-{index:03}"), &["lookup".to_owned()]).await;
    }
    let html = f.pane(f.owner, workflow, "editor").await.unwrap();
    assert!(
        html.contains("More connections offer this tool than the 20 listed here."),
        "{html}"
    );
    let select = &html[html.find(r#"name="mcp:service:lookup""#).unwrap()..];
    let select = &select[..select.find("</select>").unwrap()];
    assert_eq!(
        select.matches("<option value=\"").count(),
        21,
        "none plus 20 listed"
    );
    assert!(
        !html.contains(r#"selected>"#),
        "with several candidates nothing is preselected"
    );
}

#[tokio::test]
async fn the_editor_guards_unsaved_text_and_reports_only_the_work_it_does() {
    let Some(f) = Fixture::new().await else {
        return;
    };
    let workflow = Uuid::new_v4();
    f.draft(workflow, "Guarded", &blank_source(workflow)).await;
    let html = f.pane(f.owner, workflow, "editor").await.unwrap();
    // F003: the editor is the dirty-state owner, and its own reload is the one exempt control.
    assert!(html.contains(r#"<form id="workflow-editor" class="space-y-4" data-workflow-editor"#));
    assert!(html.contains("data-editor-reload"));
    let script = crate::adapters::http::pages::application_javascript();
    assert!(script.contains("The editor has changes that are not saved. Discard them?"));
    assert!(
        script.contains("target.id !== 'workflow-pane'"),
        "only pane replacements ask"
    );
    assert!(script.contains("beforeunload"));
    // The guard runs on `htmx:confirm`, before the shared busy-button handler (F014).
    let guard = script
        .find("addEventListener('htmx:confirm'")
        .expect("the confirm guard");
    assert!(script[guard..].contains("Discard them?"));
    assert!(!script[..guard].contains("Discard them?"));
    assert!(!html.contains("data-dirty"), "a stored draft starts clean");
    // F004: neither submit button owns a form-scoped spinner; the status says which work runs.
    let form = &html[html.find(r#"id="workflow-editor""#).unwrap()..];
    let form = &form[..form.find("</form>").unwrap()];
    assert!(!form.contains("[.htmx-request_&]"), "{form}");
    assert!(form.contains(r#"id="workflow-editor-status""#));
    assert!(script.contains("'Saving…' : 'Saving and validating…'"));
    assert!(html.contains(r##"hx-target="#workflow-reference-body" hx-swap="outerHTML""##));
    let Html(reference) = catalogue(f.ui(f.owner), f.query()).await.unwrap();
    assert!(
        reference.starts_with(r#"<div id="workflow-reference-body""#),
        "{reference}"
    );
    assert!(
        !reference.contains("aria-busy"),
        "the loaded reference is not busy"
    );
}

#[tokio::test]
async fn a_source_and_description_that_start_with_a_newline_keep_it() {
    let Some(f) = Fixture::new().await else {
        return;
    };
    let workflow = Uuid::new_v4();
    let source = format!("\n{}", blank_source(workflow));
    save_draft(
        &f.engine,
        f.company,
        WorkflowActor::authenticated(f.owner).unwrap(),
        workflow,
        None,
        DraftContent {
            title: "Leading".into(),
            description: "\nsecond line".into(),
            source: source.clone(),
        },
    )
    .await
    .unwrap();
    let html = f.pane(f.owner, workflow, "editor").await.unwrap();
    // The parser drops one newline after the start tag; the markup puts it there itself.
    assert!(
        html.contains("aria-describedby=\"workflow-source-help\">\n\nformat_version"),
        "{html}"
    );
    assert!(html.contains("maxlength=\"4096\">\n\nsecond line"));
}

#[tokio::test]
async fn a_draft_that_moves_after_the_save_keeps_the_text_and_says_both() {
    let Some(f) = Fixture::new().await else {
        return;
    };
    let workflow = Uuid::new_v4();
    let source = blank_source(workflow);
    f.draft(workflow, "Moving", &source).await;
    // Someone saves revision 2 between the author's save (revision 1 here) and its validation.
    let theirs = source.replace("max_steps: 10", "max_steps: 11");
    save_draft(
        &f.engine,
        f.company,
        WorkflowActor::authenticated(f.admin).unwrap(),
        workflow,
        Some(1),
        DraftContent {
            title: "Moving".into(),
            description: String::new(),
            source: theirs,
        },
    )
    .await
    .unwrap();
    let mine = source.replace("max_steps: 10", "max_steps: 12");
    let state = validated(
        &f.ui(f.owner),
        f.company,
        workflow,
        1,
        Typed {
            title: "Moving".into(),
            description: String::new(),
            source: mine,
            expected_revision: 1,
        },
        pages::WorkflowSelections::default(),
    )
    .await
    .unwrap();
    let html = f
        .ui(f.owner)
        .pane_html(f.company, workflow, pages::WorkflowTab::Editor, state)
        .await
        .unwrap();
    assert!(
        html.contains(
            "Saved revision 1, but someone else saved revision 2 before it could be validated."
        ),
        "{html}"
    );
    assert!(html.contains("max_steps: 12"), "the author's text stays");
    assert_eq!(field_value(&html, "expected_revision"), "2");
}

#[tokio::test]
async fn a_save_refreshes_the_sidebar_and_a_changed_publication_is_flagged() {
    let Some(f) = Fixture::new().await else {
        return;
    };
    let workflow = Uuid::new_v4();
    f.draft(workflow, "Before", &blank_source(workflow)).await;
    let html = f
        .save(
            workflow,
            &[
                ("expected_revision", "1"),
                ("title", "After"),
                ("source", &blank_source(workflow)),
                ("intent", "save"),
            ],
        )
        .await
        .unwrap();
    let menu = &html[html
        .find(r#"<ul id="workflow-menu" hx-swap-oob="true""#)
        .expect("the sidebar refresh")..];
    assert!(
        menu.contains("After") && menu.contains("draft revision 2"),
        "{menu}"
    );
    let published = text(
        publish(
            f.ui(f.owner),
            Path(workflow),
            f.query(),
            form(&[
                ("expected_revision", "2"),
                ("command_key", "flagged"),
                ("previewed_dependencies", &"0".repeat(64)),
            ]),
        )
        .await
        .unwrap(),
    )
    .await;
    assert!(
        published.contains("Its content differs from the preview you validated"),
        "{published}"
    );
}

#[tokio::test]
async fn a_draft_naming_more_tools_than_are_offered_still_saves_and_renders() {
    let Some(f) = Fixture::new().await else {
        return;
    };
    let workflow = Uuid::new_v4();
    let mut source = example("tool.call", workflow);
    let template = source["steps"]["start"].clone();
    let count = crate::application::workflow::reads::MAX_OPTION_NAMES + 1;
    let mut steps = serde_json::Map::new();
    for index in 0..count {
        let mut step = template.clone();
        step["with"]["tool"] = json!({"literal": format!("tool_{index:03}")});
        let next = if index + 1 == count {
            "$end".to_owned()
        } else {
            format!("s{:03}", index + 1)
        };
        step["routes"] = json!({ "success": next });
        steps.insert(format!("s{index:03}"), step);
    }
    source["steps"] = serde_json::Value::Object(steps);
    source["entry"] = json!("s000");
    let source = serde_json::to_string(&source).unwrap();
    f.draft(workflow, "Many", &source).await;
    // Before F013 the options read refused the draft and the editor could not open.
    let html = f.pane(f.owner, workflow, "editor").await.unwrap();
    assert!(html.contains(r#"id="workflow-editor""#), "{html}");
    assert!(html.contains("This draft names more than 256 tools of one kind"));
    let saved = f
        .save(
            workflow,
            &[
                ("expected_revision", "1"),
                ("title", "Many"),
                ("source", &source),
                ("intent", "save"),
            ],
        )
        .await
        .unwrap();
    assert!(saved.contains("Saved revision 2."), "{saved}");
}
