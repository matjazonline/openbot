from pathlib import Path
base=Path('src/adapters/persistence')
(base/'jsonb_test_support.rs').write_text('''//! Capture row-context diagnostics without installing a global subscriber.
use std::sync::{Arc, Mutex};
#[derive(Clone, Default)]
pub(super) struct LogCapture(Arc<Mutex<Vec<u8>>>);
impl LogCapture {
    pub(super) fn dispatch(&self) -> tracing::Dispatch {
        tracing::Dispatch::new(tracing_subscriber::fmt().with_writer(self.clone())
            .with_ansi(false).without_time().finish())
    }
    pub(super) fn text(&self) -> String {
        String::from_utf8(self.0.lock().unwrap().clone()).unwrap()
    }
}
impl std::io::Write for LogCapture {
    fn write(&mut self, bytes: &[u8]) -> std::io::Result<usize> {
        self.0.lock().unwrap().extend_from_slice(bytes);
        Ok(bytes.len())
    }
    fn flush(&mut self) -> std::io::Result<()> { Ok(()) }
}
impl<'a> tracing_subscriber::fmt::MakeWriter<'a> for LogCapture {
    type Writer = Self;
    fn make_writer(&'a self) -> Self::Writer { self.clone() }
}
''')
f=base/'mod.rs';f.write_text(f.read_text()+'\n#[cfg(test)]\nmod jsonb_test_support;\n')
f=base/'thread/tests.rs';f.write_text(f.read_text()+'\n#[path = "jsonb_row_context_tests.rs"]\nmod jsonb_row_context_tests;\n')
(base/'thread/jsonb_row_context_tests.rs').write_text('''use super::*;
use crate::adapters::persistence::jsonb_test_support::LogCapture;
use tracing::instrument::WithSubscriber;

#[tokio::test]
async fn jsonb_row_context_thread_contract_is_optional_per_message() {
    let fixture = Fixture::new("json_contract").await.unwrap();
    let mut ids = Vec::new();
    for suffix in ["broken", "valid"] {
        let message = inbound_email(fixture.thread.id,
            email_metadata(&format!("<{suffix}-{}@test.invalid>", fixture.suffix)), "{}");
        fixture.persistence.create_message(&message).await.unwrap();
        ids.push(message.id);
        let format = if suffix == "broken" { "unsupported" } else { "json_schema" };
        let value = serde_json::json!({"body":"{}", "fingerprint":"a".repeat(64),
            "contract":{"version":1,"format":format,"schema":{"type":"object"}}});
        sqlx::query("UPDATE messages SET structured_response=$1 WHERE id=$2")
            .bind(value).bind(message.id.as_uuid()).execute(&fixture.pool).await.unwrap();
    }
    let logs = LogCapture::default();
    async {
        let broken = fixture.persistence.get_thread_message(fixture.thread.id, ids[0]).await.unwrap().unwrap();
        assert!(broken.response_contract.is_none());
        assert_eq!(broken.clean_text_body, "{}");
        let valid = fixture.persistence.get_thread_message(fixture.thread.id, ids[1]).await.unwrap().unwrap();
        assert!(valid.response_contract.is_some());
        let views = fixture.persistence.list_thread_message_views(fixture.thread.id).await.unwrap();
        assert_eq!(views.len(), 2);
        assert_eq!(views.iter().filter(|view| view.response_contract.is_some()).count(), 1);
        let page = fixture.persistence.list_thread_message_views_after(fixture.thread.id, None, 100).await.unwrap();
        assert_eq!(page.len(), 2);
        assert!(fixture.persistence.get_thread_message_view(fixture.thread.id, ids[0]).await.unwrap().unwrap().response_contract.is_none());
    }.with_subscriber(logs.dispatch()).await;
    assert!(logs.text().contains(&ids[0].as_uuid().to_string()));
    assert!(logs.text().contains("unsupported"));
    fixture.cleanup().await;
}
''')
f=base/'mcp_tests.rs';f.write_text(f.read_text()+'''
#[tokio::test]
async fn jsonb_row_context_mcp_quarantines_discovery_and_effective_grants() {
    use super::jsonb_test_support::LogCapture;
    use tracing::instrument::WithSubscriber;
    let pool = test_pool().await.unwrap();
    let p = PostgresPersistence::with_credential_cipher(pool.clone(), CredentialCipher::for_test());
    let company = company(&p).await;
    let broken = p.create_mcp_connection(company, write("broken")).await.unwrap();
    let good = p.create_mcp_connection(company, write("good")).await.unwrap();
    for discovery in [serde_json::json!([42]), serde_json::json!([{
        "name":"search", "description":"bad schema", "input_schema":42, "output_schema":null
    }])] {
        sqlx::query("UPDATE company_mcp_connections SET discovery_json=$1 WHERE company_id=$2 AND id=$3")
            .bind(discovery).bind(company).bind(broken.id).execute(&pool).await.unwrap();
        let logs = LogCapture::default();
        let connections = p.list_mcp_connections(company).with_subscriber(logs.dispatch()).await.unwrap();
        assert_eq!(connections.len(), 2);
        let quarantined = connections.iter().find(|row| row.id == broken.id).unwrap();
        assert!(quarantined.discovered_tools.is_empty());
        assert!(quarantined.tool_grants.is_empty());
        let neighbor = connections.iter().find(|row| row.id == good.id).unwrap();
        assert_eq!(neighbor.discovered_tools.len(), 1);
        assert_eq!(neighbor.tool_grants.len(), 1);
        let stored: i64 = sqlx::query_scalar("SELECT count(*) FROM company_mcp_tool_grants WHERE company_id=$1 AND connection_id=$2")
            .bind(company).bind(broken.id).fetch_one(&pool).await.unwrap();
        assert_eq!(stored, 1);
        assert!(logs.text().contains(&broken.id.to_string()));
    }
    sqlx::query("DELETE FROM companies WHERE id=$1").bind(company).execute(&pool).await.unwrap();
}
''')
f=base/'delivery/tests.rs';f.write_text(f.read_text()+'''
#[tokio::test]
async fn jsonb_row_context_delivery_reports_id_before_creating_queue_rows() {
    let database = own_database().await.unwrap();
    let p = PostgresPersistence::new(database.pool.clone());
    let scope = scope(&p).await;
    let fixture = delivery_fixture(&p, DeliveryFixtureRequest::new(
        scope.company.id, scope.channel.id, scope.thread_id, "json-broken")).await;
    sqlx::query("UPDATE messages SET structured_response=jsonb_build_object('body',clean_text_body,'fingerprint',repeat('a',64),'contract',jsonb_build_object('version',1,'format','unsupported','schema','{}'::jsonb)) WHERE id=$1")
        .bind(fixture.delivery.message_id.as_uuid()).execute(p.pool()).await.unwrap();
    let mut tx = p.pool().begin().await.unwrap();
    let error = enqueue::insert_delivery_on(&mut tx, &fixture.delivery).await.unwrap_err();
    assert!(matches!(error, crate::app_error::AppError::Internal(_)));
    assert!(error.to_string().contains(&fixture.delivery.id.as_uuid().to_string()));
    assert!(error.to_string().contains("Unsupported response contract"));
    tx.rollback().await.unwrap();
    let count: i64 = sqlx::query_scalar("SELECT count(*) FROM message_deliveries")
        .fetch_one(p.pool()).await.unwrap();
    assert_eq!(count, 0);
    let count: i64 = sqlx::query_scalar("SELECT count(*) FROM message_delivery_parts")
        .fetch_one(p.pool()).await.unwrap();
    assert_eq!(count, 0);
    let good = queue(&p, &scope, DeliveryFixtureRequest::new(
        scope.company.id, scope.channel.id, scope.thread_id, "json-good")).await;
    assert_eq!(status_of(&p, good.delivery.id).await, DeliveryStatus::Pending);
}
''')
f=base/'workflow/harness_store_tests.rs';f.write_text(f.read_text()+'''
#[tokio::test]
async fn jsonb_row_context_checkpoint_reports_persisted_id_and_preserves_neighbor() {
    use crate::application::workflow::checkpoints::WorkflowCheckpointStore;
    let NativeFixture { fixture: f, fence, mut row } = native_row().await;
    let policy = LeasePolicy::new(Duration::from_secs(60)).unwrap();
    let mut spec = specification(&row);
    let command = f.prepare(f.request("valid-json-neighbor", f.manual())).await;
    f.persistence().admit(&command).await.unwrap();
    let job = sqlx::query_scalar("SELECT id FROM background_tasks WHERE workflow_execution_id=$1")
        .bind(command.first_execution_id().as_uuid()).fetch_one(f.persistence().pool()).await.unwrap();
    let scope = ActivationRequest { company: command.company_id(), run: command.proposed_run_id(),
        execution: command.first_execution_id(), job: WorkflowJobId(job) };
    let neighbor = f.persistence().settled().claim_io(scope, worker(), policy).await.unwrap().unwrap();
    spec.identity.task_id = job;
    let good = checkpoint(f.persistence().open_workflow_checkpoint(neighbor.fence, &spec, policy).await.unwrap());
    row["checkpoint"]["messages"][0] = json!(42);
    let mut tx = f.persistence().pool().begin().await.unwrap();
    insert(&mut tx, &row).await.unwrap();
    tx.commit().await.unwrap();
    let error = f.persistence().load_workflow_checkpoint(fence, policy).await.unwrap_err();
    assert!(matches!(error, AppError::Internal(_)));
    assert!(error.to_string().contains(row["id"].as_str().unwrap()));
    assert!(error.to_string().contains("invalid type"));
    let loaded = checkpoint(f.persistence().load_workflow_checkpoint(neighbor.fence, policy).await.unwrap());
    assert_eq!(loaded, good);
}
''')
p=Path('plan/fix-app/phases/p08/docs');note='\n\n## Resume checkpoint D5\nD4 correction gate passed31/0/0 with all5 filters nonzero; original235pass/3ignored unchanged. D5 core row-aware decoders implemented; native jsonb_row_context_ tests now written, compilation/native verification pending. No code acceptance; G1 unchanged. Next complete D5 focused checks then constant queue columns originalstep2 LAST.\n'
for name in ['TASK.md','HANDOFF.md']:
 f=p/name; f.write_text(f.read_text()+note)
