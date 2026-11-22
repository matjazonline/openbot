//! `unit_archive_` runs (Plan08 02B.1–2): an archive refuses while its desks have open work or
//! cancels it on request, an archived unit's desks admit no run by any writer, and admission
//! racing an archive settles on the company lock.
use super::*;
use crate::adapters::persistence::test_support::{
    department_use_cases, wait_until_backends_are_blocked,
};
use crate::application::ports::department::OpenWork;
use crate::entities::department::{Department, DepartmentId};
use crate::use_cases::department::DepartmentUseCases;

/// A unit under a parent unit, with a desk of its own, over a binding whose runs claim an I/O job.
struct Archive {
    f: AdmissionFixture,
    company: Uuid,
    owner: Uuid,
    parent: Uuid,
    unit: Uuid,
    desk: Uuid,
    departments: Arc<DepartmentUseCases>,
}

impl Archive {
    async fn new() -> Self {
        let f =
            AdmissionFixture::with_binding(BindingFixture::from_source(lease_source()).await).await;
        let company = f.binding.target.company.as_uuid();
        let owner = f.binding.target.actor.user_id();
        let p = f.persistence();
        let parent = p.test_unit(company, "parent").await;
        let unit = sqlx::query_scalar(
            "INSERT INTO departments (id, company_id, parent_id, name, slug, created_by) \
             VALUES (gen_random_uuid(), $1, $2, 'archived', 'archived', \
             '{\"actor_type\":\"system\",\"actor_id\":null,\"actor_name\":\"Fixture\"}') RETURNING id",
        )
        .bind(company)
        .bind(parent)
        .fetch_one(p.pool())
        .await
        .unwrap();
        let desk = crate::adapters::persistence::test_support::create_channel(
            p,
            company,
            crate::application::ports::channel::ChannelWrite {
                name: "Unit desk".into(),
                slug: format!("desk-{}", Uuid::new_v4().simple()),
                department_id: Some(DepartmentId::new(unit)),
                ..Default::default()
            },
        )
        .await
        .unwrap()
        .id;
        Self {
            departments: department_use_cases(p),
            f,
            company,
            owner,
            parent,
            unit,
            desk,
        }
    }
    fn p(&self) -> &PostgresPersistence {
        self.f.persistence()
    }
    fn request(&self, actor: WorkflowActor) -> AdmitWorkflowRequest {
        let mut request = self.f.request(&Uuid::new_v4().to_string(), self.f.manual());
        request.actor = actor;
        request.association = RelatedAssociation::Channel(
            crate::application::workflow::RelatedChannelId::new(self.desk),
        );
        request
    }
    /// The service's checks and command for `actor`, or its refusal.
    async fn prepare(&self, actor: WorkflowActor) -> AppResult<PreparedAdmission> {
        let capture = Capture::default();
        WorkflowService::new(
            self.p().clone(),
            capture.clone(),
            NoRuntime,
            NoRuntime,
            Preflight,
        )
        .admit(self.request(actor))
        .await?;
        Ok(capture.0.lock().unwrap().take().unwrap())
    }
    /// Admit a run on the unit's desk as `actor`: the run's scope, or the refusal.
    async fn admit_as(&self, actor: WorkflowActor) -> AppResult<ActivationRequest> {
        let command = self.prepare(actor).await?;
        self.p().admit(&command).await?;
        Ok(self.scope(&command).await)
    }
    async fn admit(&self) -> ActivationRequest {
        self.admit_as(self.f.binding.target.actor).await.unwrap()
    }
    async fn scope(&self, command: &PreparedAdmission) -> ActivationRequest {
        ActivationRequest {
            company: command.company_id(),
            run: command.proposed_run_id(),
            execution: command.first_execution_id(),
            job: WorkflowJobId(
                sqlx::query_scalar(
                    "SELECT id FROM background_tasks WHERE workflow_execution_id=$1",
                )
                .bind(command.first_execution_id().as_uuid())
                .fetch_one(self.p().pool())
                .await
                .unwrap(),
            ),
        }
    }
    async fn archive_as(&self, user: Uuid, open: OpenWork) -> AppResult<Department> {
        self.departments
            .archive_department(user, self.company, DepartmentId::new(self.unit), open)
            .await
    }
    async fn archive(&self, open: OpenWork) -> AppResult<Department> {
        self.archive_as(self.owner, open).await
    }
    async fn restore(&self) -> Department {
        self.departments
            .restore_department(self.owner, self.company, DepartmentId::new(self.unit))
            .await
            .unwrap()
    }
    async fn archived(&self) -> bool {
        sqlx::query_scalar("SELECT archived_at IS NOT NULL FROM departments WHERE id=$1")
            .bind(self.unit)
            .fetch_one(self.p().pool())
            .await
            .unwrap()
    }
    async fn runs(&self) -> i64 {
        sqlx::query_scalar(
            "SELECT count(*) FROM workflow_runs WHERE company_id=$1 AND channel_id=$2",
        )
        .bind(self.company)
        .bind(self.desk)
        .fetch_one(self.p().pool())
        .await
        .unwrap()
    }
    async fn open_runs(&self) -> i64 {
        sqlx::query_scalar(
            "SELECT count(*) FROM workflow_runs WHERE company_id=$1 AND channel_id=$2 \
             AND state IN ('queued','running','waiting')",
        )
        .bind(self.company)
        .bind(self.desk)
        .fetch_one(self.p().pool())
        .await
        .unwrap()
    }
    async fn state(&self, scope: ActivationRequest) -> String {
        sqlx::query_scalar("SELECT state FROM workflow_runs WHERE id=$1")
            .bind(scope.run.as_uuid())
            .fetch_one(self.p().pool())
            .await
            .unwrap()
    }
    /// The archive's cancel events on `scope`'s run, with the acting user.
    async fn cancel_events(&self, scope: ActivationRequest) -> Vec<Option<Uuid>> {
        sqlx::query_scalar(
            "SELECT actor_id FROM workflow_run_events WHERE company_id=$1 AND run_id=$2 \
             AND event_kind='control_cancel' ORDER BY sequence",
        )
        .bind(self.company)
        .bind(scope.run.as_uuid())
        .fetch_all(self.p().pool())
        .await
        .unwrap()
    }
}

fn refused_by(error: sqlx::Error) -> String {
    error
        .as_database_error()
        .and_then(|database| database.constraint().map(str::to_string))
        .unwrap_or_else(|| panic!("a named constraint, not {error:?}"))
}

#[tokio::test]
async fn unit_archive_refuses_open_work_and_changes_nothing() {
    let a = Archive::new().await;
    let run = a.admit().await;
    let refused = a.archive(OpenWork::Refuse).await;
    assert!(
        matches!(
            refused,
            Err(AppError::UnitArchiveOpenWork {
                count: 1,
                cancellable: true
            })
        ),
        "{refused:?}"
    );
    assert!(!a.archived().await);
    assert_eq!(a.state(run).await, "queued");
    assert!(a.cancel_events(run).await.is_empty());
}

/// U3: the parent's manager ends the subtree's open runs through cancel-and-archive, and each
/// cancellation is recorded with that manager as actor.
#[tokio::test]
async fn unit_archive_cancel_by_the_parent_manager_ends_open_runs_and_records_who() {
    let a = Archive::new().await;
    let lead = a.p().test_unit_lead(a.company, a.parent).await;
    let first = a.admit().await;
    let second = a.admit().await;
    let archived = a
        .archive_as(lead, OpenWork::Cancel)
        .await
        .expect("the parent's lead cancels and archives");
    assert!(archived.is_archived());
    for run in [first, second] {
        assert_eq!(a.state(run).await, "cancelled");
        assert_eq!(a.cancel_events(run).await, vec![Some(lead)]);
    }
    assert_eq!(a.open_runs().await, 0);
}

/// Cancel-and-archive stays bounded: past `MAX_ARCHIVE_CANCELLATIONS` root runs it refuses as not
/// cancellable and cancels nothing.
#[tokio::test]
async fn unit_archive_cancel_refuses_past_its_bound_and_cancels_nothing() {
    let a = Archive::new().await;
    let bound = crate::adapters::persistence::department::MAX_ARCHIVE_CANCELLATIONS;
    for _ in 0..=bound {
        a.admit().await;
    }
    let refused = a.archive(OpenWork::Cancel).await;
    assert!(
        matches!(
            refused,
            Err(AppError::UnitArchiveOpenWork { count, cancellable: false }) if count == bound + 1
        ),
        "{refused:?}"
    );
    assert!(!a.archived().await);
    assert_eq!(a.open_runs().await, bound + 1);
}

/// Plan08 02B.2 (D1.5): admission on an archived unit's desk refuses with `unit.archived`, also
/// for the unit's own lead whose role the archive suspended, and admits again after a restore.
#[tokio::test]
async fn unit_archive_runs_admission_refuses_on_an_archived_desk_and_resumes_after_restore() {
    let a = Archive::new().await;
    let lead = a.p().test_unit_lead(a.company, a.unit).await;
    let lead = WorkflowActor::authenticated(lead).unwrap();
    // This binding is the root's, so the lead's admission is refused by authority while the
    // unit is active; once it is archived the archive check, which runs first, answers instead.
    let active = a.admit_as(lead).await;
    assert!(matches!(active, Err(AppError::NotFound(_))), "{active:?}");
    let run = a.admit().await;
    a.archive(OpenWork::Cancel).await.unwrap();
    assert_eq!(a.state(run).await, "cancelled");
    let before = a.runs().await;
    for actor in [a.f.binding.target.actor, lead] {
        let refused = a.admit_as(actor).await;
        assert!(
            matches!(refused, Err(AppError::UnitArchived)),
            "{refused:?}"
        );
    }
    assert_eq!(a.runs().await, before, "no run was written");
    // A command prepared before the archive is refused by the persistence step itself.
    a.restore().await;
    let prepared = a.prepare(a.f.binding.target.actor).await.unwrap();
    a.archive(OpenWork::Refuse).await.unwrap();
    assert!(matches!(
        a.p().admit(&prepared).await,
        Err(AppError::UnitArchived)
    ));
    a.restore().await;
    a.admit().await;
    assert_eq!(a.open_runs().await, 1);
}

/// Plan08 02B.2: an operator retry cannot reopen a failed run on an archived desk, and the schema
/// refuses any writer that inserts or reopens a run there.
#[tokio::test]
async fn unit_archive_runs_retry_and_the_run_trigger_refuse_on_an_archived_desk() {
    let a = Archive::new().await;
    let run = a.admit().await;
    Box::pin(control_tests::failed(
        &a.f,
        run,
        crate::domain::workflow::RetrySafety::SafeToRetry,
    ))
    .await;
    assert_eq!(a.state(run).await, "failed");
    a.archive(OpenWork::Refuse)
        .await
        .expect("a failed run is no open work");
    let command = control_tests::command(&a.f, run, "archived-retry").await;
    let retried = a.p().retry(control_tests::retry(command)).await;
    assert!(
        matches!(retried, Err(AppError::UnitArchived)),
        "{retried:?}"
    );
    assert_eq!(a.state(run).await, "failed");
    // A writer that skips the retry path but satisfies the transition guard (reopened job,
    // cleared terminal execution) is still refused by the unit backstop.
    let mut writer = a.p().pool().begin().await.unwrap();
    sqlx::query(
        "UPDATE background_tasks SET status='pending' WHERE workflow_execution_id = \
         (SELECT terminal_execution_id FROM workflow_runs WHERE id=$1)",
    )
    .bind(run.run.as_uuid())
    .execute(&mut *writer)
    .await
    .unwrap();
    let reopened = sqlx::query(
        "UPDATE workflow_runs SET state='running', terminal_execution_id=NULL WHERE id=$1",
    )
    .bind(run.run.as_uuid())
    .execute(&mut *writer)
    .await
    .unwrap_err();
    writer.rollback().await.unwrap();
    assert_eq!(refused_by(reopened), "unit_archived");
    let inserted = sqlx::query(
        "INSERT INTO workflow_runs SELECT (jsonb_populate_record(NULL::workflow_runs, to_jsonb(run) \
         || jsonb_build_object('id', gen_random_uuid(), 'state', 'queued', 'finished_at', NULL))).* \
         FROM workflow_runs AS run WHERE id=$1",
    )
    .bind(run.run.as_uuid())
    .execute(a.p().pool())
    .await
    .unwrap_err();
    assert_eq!(refused_by(inserted), "unit_archived");
    // After a restore the same retry applies.
    a.restore().await;
    let command = control_tests::command(&a.f, run, "restored-retry").await;
    assert!(matches!(
        a.p().retry(control_tests::retry(command)).await.unwrap(),
        RetryResult::Applied { .. }
    ));
}

/// Admission and archive released together from behind the company lock: either the run is
/// admitted and the archive refuses for it, or the archive commits and admission refuses; never
/// an open run on an archived unit.
#[tokio::test]
async fn unit_archive_competing_admission_and_archive_agree() {
    let a = Arc::new(Archive::new().await);
    let prepared = a.prepare(a.f.binding.target.actor).await.unwrap();
    let mut blocker = a.p().pool().begin().await.unwrap();
    sqlx::query("SELECT 1 FROM companies WHERE id=$1 FOR NO KEY UPDATE")
        .bind(a.company)
        .execute(&mut *blocker)
        .await
        .unwrap();
    let admission = tokio::spawn({
        let a = a.clone();
        async move { a.p().admit(&prepared).await }
    });
    let archive = tokio::spawn({
        let a = a.clone();
        async move { a.archive(OpenWork::Refuse).await }
    });
    wait_until_backends_are_blocked(a.p().pool(), 2).await;
    blocker.commit().await.unwrap();
    let (admitted, archived) = (admission.await.unwrap(), archive.await.unwrap());
    match (&admitted, &archived) {
        (Ok(_), Err(AppError::UnitArchiveOpenWork { count: 1, .. })) => {
            assert!(!a.archived().await);
            assert_eq!(a.open_runs().await, 1);
        }
        (Err(AppError::UnitArchived), Ok(_)) => {
            assert!(a.archived().await);
            assert_eq!(a.runs().await, 0);
        }
        outcome => panic!("one of them must win: {outcome:?}"),
    }
}

/// Cancel-and-archive against a run whose job a worker holds: the archive waits for the run row,
/// then cancels the run, and the worker's late result is fenced off. The run never stays open on
/// an archived unit.
#[tokio::test]
async fn unit_archive_competing_cancel_and_a_running_job() {
    let a = Arc::new(Archive::new().await);
    let run = a.admit().await;
    let claim = a
        .p()
        .settled()
        .claim_io(run, worker(), policy())
        .await
        .unwrap()
        .unwrap();
    assert_eq!(a.state(run).await, "running");
    let mut worker_tx = a.p().pool().begin().await.unwrap();
    sqlx::query("SELECT 1 FROM workflow_runs WHERE id=$1 FOR UPDATE")
        .bind(run.run.as_uuid())
        .execute(&mut *worker_tx)
        .await
        .unwrap();
    let archive = tokio::spawn({
        let a = a.clone();
        async move { a.archive(OpenWork::Cancel).await }
    });
    wait_until_backends_are_blocked(a.p().pool(), 1).await;
    worker_tx.commit().await.unwrap();
    match archive.await.unwrap() {
        Ok(department) => {
            assert!(department.is_archived());
            assert_eq!(a.state(run).await, "cancelled");
        }
        // A deadlock or lock timeout rolls the whole archive back.
        Err(AppError::Database(_)) => {
            assert!(!a.archived().await);
            assert_eq!(a.state(run).await, "running");
        }
        Err(other) => panic!("{other:?}"),
    }
    let late = a
        .p()
        .settled()
        .complete_io(FencedWorkflowResult {
            policy: policy(),
            fence: claim.fence,
            output: json!({"items":[],"token_count":0,"omitted":0}),
        })
        .await;
    assert!(matches!(late, Ok(None) | Err(_)), "{late:?}");
    assert!(!(a.archived().await && a.open_runs().await > 0));
}

fn archived_refusal<T>(result: Result<T, LifecycleError>) -> bool {
    matches!(
        result,
        Err(LifecycleError::Application(AppError::UnitArchived))
    )
}

/// Plan08 02B.4 (D1.8): a workflow binding on an archived unit's desk cannot be configured or
/// activated, and can still be deactivated.
#[tokio::test]
async fn unit_archive_refuses_binding_configuration_and_allows_deactivate() {
    let a = Archive::new().await;
    let target = BindingTarget {
        binding: WorkflowBindingId::new(Uuid::new_v4()),
        ..a.f.binding.target
    };
    let request = |expected| ConfigureBinding {
        target,
        association: RelatedAssociation::Channel(
            crate::application::workflow::RelatedChannelId::new(a.desk),
        ),
        ..a.f.binding.request(expected)
    };
    let service = a.f.binding.service();
    let configured = service.configure(request(None)).await.unwrap();
    let revision = || async {
        a.p()
            .binding(target.company, target.binding)
            .await
            .unwrap()
            .unwrap()
            .revision
    };
    service
        .activate(SetBindingActivity {
            target,
            expected: configured.revision,
        })
        .await
        .unwrap();
    a.archive(OpenWork::Refuse).await.unwrap();
    let reconfigured = service.configure(request(Some(revision().await))).await;
    assert!(archived_refusal(reconfigured));
    let added = service
        .configure(ConfigureBinding {
            target: BindingTarget {
                binding: WorkflowBindingId::new(Uuid::new_v4()),
                ..target
            },
            ..request(None)
        })
        .await;
    assert!(archived_refusal(added));
    service
        .deactivate(SetBindingActivity {
            target,
            expected: revision().await,
        })
        .await
        .expect("a binding on an archived unit's desk can be deactivated");
    let reactivated = service
        .activate(SetBindingActivity {
            target,
            expected: revision().await,
        })
        .await;
    assert!(archived_refusal(reactivated));
}

/// The routes as a browser reaches them: the optional body, the open-work refusal's code, count and
/// flag, `cancel_open_runs`, and `unit.archived` for a restore under an archived parent.
#[tokio::test]
async fn unit_archive_http_routes_refuse_open_work_cancel_on_request_and_restore() {
    use axum::body::{Body, to_bytes};
    use axum::http::{Request, StatusCode};
    use tower::ServiceExt;
    let a = Archive::new().await;
    let app = crate::adapters::http::routes::department::lifecycle_router()
        .with_state(a.departments.clone());
    let post = |unit: Uuid, action: &str, body: Option<&str>| {
        let builder = Request::post(format!(
            "/api/companies/{}/departments/{unit}/{action}",
            a.company
        ));
        let mut request = match body {
            Some(body) => builder
                .header("content-type", "application/json")
                .body(Body::from(body.to_owned())),
            None => builder.body(Body::empty()),
        }
        .unwrap();
        request
            .extensions_mut()
            .insert(crate::application::session::VerifiedSession {
                user_id: a.owner,
                session_id: crate::application::session::SessionId(Uuid::new_v4()),
            });
        app.clone().oneshot(request)
    };
    let read = |response: axum::response::Response| async move {
        let status = response.status();
        let body = to_bytes(response.into_body(), 65536).await.unwrap();
        (
            status,
            serde_json::from_slice::<serde_json::Value>(&body).unwrap_or_default(),
        )
    };
    let scope = a.admit().await;

    let (status, body) = read(post(a.unit, "archive", None).await.unwrap()).await;
    assert_eq!(status, StatusCode::CONFLICT);
    assert_eq!(body["code"], "unit.archive_open_work");
    assert_eq!(
        (body["count"].clone(), body["cancellable"].clone()),
        (1.into(), true.into())
    );
    assert!(!a.archived().await);
    let (status, _) = read(
        post(a.unit, "archive", Some(r#"{"cancel_open_runs":false}"#))
            .await
            .unwrap(),
    )
    .await;
    assert_eq!(status, StatusCode::CONFLICT, "false is the default");
    let (status, _) = read(
        post(a.unit, "archive", Some(r#"{"cancel":true}"#))
            .await
            .unwrap(),
    )
    .await;
    assert!(
        status.is_client_error() && status != StatusCode::CONFLICT,
        "{status}"
    );
    assert_eq!(a.open_runs().await, 1, "a rejected body cancels nothing");

    let (status, body) = read(
        post(a.unit, "archive", Some(r#"{"cancel_open_runs":true}"#))
            .await
            .unwrap(),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert!(body["department"]["archived"]["at"].is_string(), "{body}");
    assert_eq!(a.state(scope).await, "cancelled");

    let (status, body) = read(post(a.parent, "archive", None).await.unwrap()).await;
    assert_eq!(status, StatusCode::OK, "{body}");
    let (status, body) = read(post(a.unit, "restore", None).await.unwrap()).await;
    assert_eq!(status, StatusCode::CONFLICT);
    assert_eq!(body["code"], "unit.archived");
    for unit in [a.parent, a.unit] {
        let (status, body) = read(post(unit, "restore", None).await.unwrap()).await;
        assert_eq!(status, StatusCode::OK, "{body}");
        assert!(body["department"]["archived"].is_null(), "{body}");
    }
    assert!(!a.archived().await);
}
