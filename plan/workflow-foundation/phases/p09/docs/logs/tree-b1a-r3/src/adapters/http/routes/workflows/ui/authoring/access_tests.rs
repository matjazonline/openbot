//! Who reaches the workspace: hostile text, other tenants, plain members and admins, and the CSRF
//! boundary of every write.
use super::*;

#[tokio::test]
async fn hostile_text_is_escaped_in_the_list_the_editor_and_the_preview() {
    let Some(f) = Fixture::new().await else {
        return;
    };
    let workflow = Uuid::new_v4();
    // Built from parts: the HTTP sources are scanned for literal script tags (pages tests).
    let script = format!("<{}>alert(1)", "script");
    let hostile = &format!(r#"{script}</script>"><img src=x onerror=alert(2)>"#);
    let source = format!("# {hostile}\n{}", blank_source(workflow));
    f.draft(workflow, hostile, &source).await;
    let html = f
        .save(
            workflow,
            &[
                ("expected_revision", "1"),
                ("title", hostile),
                ("description", hostile),
                ("source", &source),
                ("intent", "validate"),
            ],
        )
        .await
        .unwrap();
    let Html(page) = workspace(
        f.ui(f.owner),
        Query(WorkspaceQuery {
            company_id: Some(f.company),
            workflow_id: Some(workflow),
            tab: None,
            new: None,
        }),
    )
    .await
    .unwrap();
    for rendered in [&html, &page] {
        assert!(!rendered.contains(&script), "{rendered}");
        assert!(!rendered.contains("<img src=x"), "{rendered}");
        assert!(rendered.contains("&lt;script&gt;alert(1)&lt;/script&gt;"));
    }
}

#[tokio::test]
async fn other_tenants_and_plain_members_reach_no_workflow_and_admins_do() {
    let Some(f) = Fixture::new().await else {
        return;
    };
    let workflow = Uuid::new_v4();
    f.draft(workflow, "Private", &blank_source(workflow)).await;
    for user in [f.stranger, f.member] {
        assert!(matches!(
            f.pane(user, workflow, "editor").await,
            Err(AppError::NotFound(_))
        ));
        let ui = f.ui(user);
        assert!(matches!(
            save(
                ui,
                Path(workflow),
                f.query(),
                form(&[("expected_revision", "1")])
            )
            .await,
            Err(WorkflowError::App(AppError::NotFound(_)))
        ));
        assert!(matches!(
            publish(
                f.ui(user),
                Path(workflow),
                f.query(),
                form(&[("expected_revision", "1"), ("command_key", "k")])
            )
            .await,
            Err(WorkflowError::App(AppError::NotFound(_)))
        ));
        assert!(matches!(
            archive(
                f.ui(user),
                Path(workflow),
                f.query(),
                form(&[("expected_revision", "1")])
            )
            .await,
            Err(WorkflowError::App(AppError::NotFound(_)))
        ));
        assert!(matches!(
            compare(
                f.ui(user),
                Path(workflow),
                Query(CompareQuery {
                    company_id: f.company,
                    from: "draft".into(),
                    to: "draft".into()
                })
            )
            .await,
            Err(AppError::NotFound(_))
        ));
        assert!(matches!(
            catalogue(f.ui(user), f.query()).await,
            Err(AppError::NotFound(_))
        ));
        assert!(matches!(
            list_page(
                f.ui(user),
                Query(ListQuery {
                    company_id: f.company,
                    after: None
                })
            )
            .await,
            Err(AppError::NotFound(_))
        ));
        assert!(matches!(
            new_pane(f.ui(user), f.query()).await,
            Err(AppError::NotFound(_))
        ));
        assert!(matches!(
            create_draft(
                f.ui(user),
                f.query(),
                form(&[("title", "x"), ("command_key", "k")])
            )
            .await,
            Err(WorkflowError::App(AppError::NotFound(_)))
        ));
        // A page they can open shows only companies they manage: none here.
        let Html(page) = workspace(
            f.ui(user),
            Query(WorkspaceQuery {
                company_id: Some(f.company),
                workflow_id: Some(workflow),
                tab: None,
                new: None,
            }),
        )
        .await
        .unwrap();
        assert!(!page.contains("Private"), "{page}");
    }
    // A workflow id of this company asked through another company's scope is not found.
    assert!(matches!(
        pane(
            f.ui(f.stranger),
            Path(workflow),
            Query(PaneQuery {
                company_id: f.other,
                tab: None
            })
        )
        .await,
        Err(AppError::NotFound(_))
    ));
    assert!(
        f.pane(f.admin, workflow, "editor")
            .await
            .unwrap()
            .contains("Private")
    );
}

#[tokio::test]
async fn an_admin_of_another_company_reaches_no_write_or_version_of_this_one() {
    let Some(f) = Fixture::new().await else {
        return;
    };
    let workflow = Uuid::new_v4();
    f.draft(workflow, "Victim", &source_of("data.map", workflow))
        .await;
    let own = Query(CompanyQuery {
        company_id: f.other,
    });
    let fields = [
        ("expected_revision", "1"),
        ("title", "Taken"),
        ("source", "x"),
        ("intent", "validate"),
        ("command_key", "foreign-1"),
    ];
    assert!(matches!(
        save(
            f.ui(f.stranger),
            Path(workflow),
            Query(CompanyQuery {
                company_id: f.other
            }),
            form(&fields)
        )
        .await,
        Err(WorkflowError::App(AppError::NotFound(_)))
    ));
    assert!(matches!(
        publish(
            f.ui(f.stranger),
            Path(workflow),
            Query(CompanyQuery {
                company_id: f.other
            }),
            form(&fields)
        )
        .await,
        Err(WorkflowError::App(AppError::NotFound(_)))
    ));
    assert!(matches!(
        archive(f.ui(f.stranger), Path(workflow), own, form(&fields)).await,
        Err(WorkflowError::App(AppError::NotFound(_)))
    ));
    assert!(matches!(
        compare(
            f.ui(f.stranger),
            Path(workflow),
            Query(CompareQuery {
                company_id: f.other,
                from: "draft".into(),
                to: "draft".into()
            })
        )
        .await,
        Err(AppError::NotFound(_))
    ));
    let stored = f
        .engine
        .reads()
        .definition(CompanyId::new(f.company), WorkflowId::new(workflow))
        .await
        .unwrap()
        .unwrap();
    assert_eq!(
        (stored.summary.revision, stored.summary.archived),
        (1, false)
    );
    assert!(stored.versions.is_empty() && stored.summary.title == "Victim");
    // A version of another workflow, named through this workflow's compare, is not found.
    let other = Uuid::new_v4();
    let other_source = source_of("data.map", other);
    f.draft(other, "Other", &other_source).await;
    let (version, _) = publish_draft(
        &f.engine,
        f.company,
        WorkflowActor::authenticated(f.owner).unwrap(),
        other,
        PublishRequest {
            expected_revision: 1,
            command_key: "other-1",
            http: &[],
            mcp: &[],
        },
    )
    .await
    .unwrap();
    assert!(matches!(
        compare(
            f.ui(f.owner),
            Path(workflow),
            Query(CompareQuery {
                company_id: f.company,
                from: version.as_uuid().to_string(),
                to: "draft".into()
            })
        )
        .await,
        Err(AppError::NotFound(_))
    ));
}

#[tokio::test]
async fn every_workspace_write_refuses_a_cross_origin_cookie_post() {
    let mut app = Router::new();
    for pattern in WRITE_ROUTES {
        app = app.route(pattern, post(|| async { StatusCode::NO_CONTENT }));
    }
    let app = app.layer(axum::middleware::from_fn_with_state(
        crate::adapters::http::routes::ui::stream_tests::test_config(),
        crate::adapters::http::security::browser_security,
    ));
    for pattern in WRITE_ROUTES {
        let path = pattern
            .replace("{workflow_id}", &Uuid::new_v4().to_string())
            .replace("{template_id}", "reviewed-support");
        let request = |origin: &str| {
            Request::post(path.as_str())
                .header(header::HOST, "localhost")
                .header(header::COOKIE, "session=signed")
                .header(header::ORIGIN, origin)
                .body(axum::body::Body::empty())
                .unwrap()
        };
        let foreign = app
            .clone()
            .oneshot(request("https://foreign.example"))
            .await
            .unwrap();
        assert_eq!(foreign.status(), StatusCode::FORBIDDEN, "{path}");
        let same = app
            .clone()
            .oneshot(request("http://localhost"))
            .await
            .unwrap();
        assert_eq!(same.status(), StatusCode::NO_CONTENT, "{path}");
    }
}
