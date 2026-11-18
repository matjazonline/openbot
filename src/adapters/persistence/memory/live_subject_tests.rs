//! OpenBot delivery and the actual operator process against the real service.
//! Source classification is supplied by the existing trusted freeze fixture; the
//! HTTP transport, persisted subject, pagination and CLI cleanup run unchanged.
use super::*;
use crate::application::ports::department::DepartmentWrite;
use crate::entities::department::DepartmentId;
use crate::entities::transport::PrincipalId;
use base64::Engine as _;
use sqlx::ConnectOptions;

async fn unit(f: &Fixture) -> DepartmentId {
    crate::adapters::persistence::test_support::department_use_cases(&f.persistence)
        .create_department(
            f.owner,
            f.company,
            DepartmentWrite {
                name: "Subject unit".into(),
                slug: "subject-unit".into(),
                ..Default::default()
            },
            None,
        )
        .await
        .unwrap()
        .id
}

async fn seed(f: &Fixture, owner: NamespaceOwner, subject: PrincipalId, count: usize) {
    let facts = vec!["A subject preference.".to_owned(); count];
    let key = format!("subject-{}", Uuid::new_v4().simple());
    let intent = SaveIntent {
        company: f.scope(),
        run: RunId::new(Uuid::new_v4()),
        execution: ExecutionId::new(Uuid::new_v4()),
        invocation: None,
        wait: None,
        scope: owner,
        key: &key,
        facts: &facts,
        producer: f.agent_principal,
        subject: Some(subject),
        external_sources: Vec::new(),
    };
    let mut tx = f.pool().begin().await.unwrap();
    assert!(matches!(
        freeze_save_on(
            &mut tx,
            &intent,
            LockedRun {
                ceiling: AccessLevel::L6,
                content: AccessLevel::L3,
                content_exact: true,
            },
        )
        .await
        .unwrap(),
        SaveEnqueue::Enqueued { .. }
    ));
    tx.commit().await.unwrap();
}

async fn command(f: &Fixture, live: &Live, operation: &str, token: &str) -> (bool, Value) {
    let binary = std::env::var("MEMORY_LIVE_OPENBOT_BIN")
        .expect("scripts/memory-integration.sh builds the OpenBot operator binary");
    let mut process = tokio::process::Command::new(binary);
    process
        .args(["memory", "subject", operation, "--company"])
        .arg(f.company.to_string())
        .arg("--subject")
        .arg(f.member_principal.as_uuid().to_string())
        .env(
            "DATABASE_URL",
            f.pool().connect_options().to_url_lossy().as_str(),
        )
        .env("MEMORY_SERVICE_URL", &live.url)
        .env("MEMORY_SERVICE_PROVISIONER_TOKEN", token)
        .env(
            "CREDENTIAL_ENCRYPTION_KEYS",
            format!(
                "1:{}",
                base64::engine::general_purpose::STANDARD.encode([7_u8; 32])
            ),
        )
        .env("CREDENTIAL_ENCRYPTION_ACTIVE_VERSION", "1")
        .kill_on_drop(true);
    let output = tokio::time::timeout(Duration::from_secs(70), process.output())
        .await
        .expect("operator command is bounded")
        .unwrap();
    let summary: Value = String::from_utf8(output.stdout)
        .unwrap()
        .lines()
        .filter_map(|line| serde_json::from_str::<Value>(line).ok())
        .next_back()
        .expect("CLI emits its completion summary on success and failure");
    (output.status.success(), summary)
}

#[tokio::test]
#[ignore = "needs the live memory service and operator binary: scripts/memory-integration.sh"]
async fn memory_e2e_subject_delivery_and_operator_purge_page_units_and_preserve_personal_records() {
    use secrecy::ExposeSecret;

    let live = live();
    let http = client(&live);
    let f = Fixture::new().await.unwrap();
    e2e::provisioned(&f, &live, &http).await;
    let child = unit(&f).await;
    f.ensure(NamespaceOwner::Unit(child)).await;
    reconcile(&f, &live, &http).await;
    for _ in 0..13 {
        seed(&f, NamespaceOwner::Unit(f.root), f.member_principal, 8).await;
    }
    seed(&f, NamespaceOwner::Unit(child), f.member_principal, 1).await;
    seed(&f, NamespaceOwner::Unit(f.root), f.admin_principal, 1).await;
    seed(
        &f,
        NamespaceOwner::Principal(f.agent_principal),
        f.member_principal,
        1,
    )
    .await;
    deliver(&f, &http).await;
    let pending: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM memory_deliveries WHERE company_id=$1 AND state<>'delivered'",
    )
    .bind(f.company)
    .fetch_one(f.pool())
    .await
    .unwrap();
    assert_eq!(
        pending, 0,
        "all subject payloads reached the actual service"
    );
    let token = live.provisioner.secret().expose_secret();
    let (ok, list) = command(&f, &live, "list", token).await;
    assert!(ok, "{list}");
    assert_eq!(list["complete"], true);
    assert_eq!(list["progress"]["namespaces"], 2);
    assert_eq!(list["progress"]["listed"], 105);
    let invalid = format!("msk_adm_{}_{}", Uuid::new_v4().simple(), "A".repeat(43));
    let (ok, failure) = command(&f, &live, "purge", &invalid).await;
    assert!(!ok);
    assert_eq!(failure["complete"], false);
    assert_eq!(failure["progress"]["deleted"], 0);
    let (ok, purge) = command(&f, &live, "purge", token).await;
    assert!(ok, "{purge}");
    assert_eq!(purge["complete"], true);
    assert_eq!(purge["progress"]["deleted"], 105);
    let (ok, retry) = command(&f, &live, "purge", token).await;
    assert!(ok, "{retry}");
    assert_eq!(retry["progress"]["deleted"], 0);
    for (owner, subject) in [
        (NamespaceOwner::Unit(f.root), f.admin_principal),
        (
            NamespaceOwner::Principal(f.agent_principal),
            f.member_principal,
        ),
    ] {
        let (namespace, _) = ready(&f, owner).await;
        let page = http
            .list_subject(
                MemorySubjectScope {
                    tenant: TenantExternalId(f.company),
                    namespace: NamespaceExternalId(namespace),
                    subject,
                },
                None,
            )
            .await
            .unwrap();
        assert_eq!(
            page.items.len(),
            1,
            "another subject and personal data survive"
        );
    }
}
