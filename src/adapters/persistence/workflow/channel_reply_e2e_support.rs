use super::*;
use crate::adapters::protocols::email::{OUTBOUND_EMAIL_VERSION, OutboundEmailV1};
use crate::application::services::delivery_worker::DeliveryWorker;
use crate::entities::transport::{DeliveryId, DeliveryPurpose, FailureClass, TransportKind};
use crate::transport::{
    DeliveryRecord, FailureDetail, ProviderSendOutcome, RenderedPart, TransportRegistry,
    TransportSender,
};
use std::collections::VecDeque;
use std::sync::Mutex;

#[derive(Clone)]
pub(super) struct Sent {
    pub delivery: DeliveryId,
    pub email: OutboundEmailV1,
}

#[derive(Default)]
pub(super) struct Provider {
    outcomes: Mutex<VecDeque<ProviderSendOutcome>>,
    pub sent: Mutex<Vec<Sent>>,
    acknowledgement: Acknowledgement,
}

#[derive(Default)]
enum Acknowledgement {
    #[default]
    Returned,
    Lost,
}

impl Provider {
    pub fn messages(&self) -> Vec<Sent> {
        self.sent.lock().unwrap().clone()
    }
}

#[async_trait]
impl TransportSender for Provider {
    fn transport(&self) -> TransportKind {
        TransportKind::Email
    }

    async fn send(&self, delivery: &DeliveryRecord, part: &RenderedPart) -> ProviderSendOutcome {
        if delivery.purpose != DeliveryPurpose::WorkflowReply {
            return ProviderSendOutcome::Delivered { provider_key: None };
        }
        self.sent.lock().unwrap().push(Sent {
            delivery: delivery.id,
            email: part
                .payload
                .decode(TransportKind::Email, OUTBOUND_EMAIL_VERSION)
                .unwrap(),
        });
        if matches!(self.acknowledgement, Acknowledgement::Lost) {
            // The counted remote side accepted the wire identity, but its response
            // never reaches the worker. Shutdown must preserve this ambiguity.
            std::future::pending::<()>().await;
        }
        self.outcomes
            .lock()
            .unwrap()
            .pop_front()
            .unwrap_or(ProviderSendOutcome::Delivered { provider_key: None })
    }
}

pub(super) enum ProviderCase {
    Accept,
    Retry,
    Unknown,
    LostReceipt,
}

pub(super) struct Delivery {
    pub provider: Arc<Provider>,
    shutdown: tokio::sync::broadcast::Sender<()>,
    handle: Option<tokio::task::JoinHandle<()>>,
}

impl Delivery {
    pub fn start(d: &Deployment, case: ProviderCase) -> Self {
        let outcomes = match case {
            ProviderCase::Accept | ProviderCase::LostReceipt => VecDeque::new(),
            ProviderCase::Retry => VecDeque::from([ProviderSendOutcome::RetryAfter {
                retry_after: Duration::ZERO,
                class: FailureClass::Timeout,
                detail: FailureDetail::parse("Definitely refused; retry allowed").unwrap(),
            }]),
            ProviderCase::Unknown => VecDeque::from([ProviderSendOutcome::OutcomeUnknown {
                class: FailureClass::Timeout,
                detail: FailureDetail::parse("Accepted remotely; acknowledgement lost").unwrap(),
            }]),
        };
        let provider = Arc::new(Provider {
            outcomes: Mutex::new(outcomes),
            acknowledgement: if matches!(case, ProviderCase::LostReceipt) {
                Acknowledgement::Lost
            } else {
                Acknowledgement::Returned
            },
            ..Provider::default()
        });
        let registry = TransportRegistry::new()
            .register(Arc::new(EmailRenderer::new(APP_DOMAIN)), provider.clone())
            .unwrap();
        let worker = Arc::new(DeliveryWorker::new(
            Arc::new(d.p().clone()),
            Arc::new(registry),
        ));
        let (shutdown, receiver) = tokio::sync::broadcast::channel(1);
        Self {
            provider,
            shutdown,
            handle: Some(tokio::spawn(worker.run(receiver))),
        }
    }

    pub async fn stop(mut self) -> Arc<Provider> {
        let _ = self.shutdown.send(());
        tokio::time::timeout(Duration::from_secs(5), self.handle.take().unwrap())
            .await
            .unwrap()
            .unwrap();
        self.provider.clone()
    }
}

impl Drop for Delivery {
    fn drop(&mut self) {
        if let Some(handle) = &self.handle {
            handle.abort();
        }
    }
}

pub(super) fn example(index: usize) -> ResponseExample {
    response_examples().remove(index)
}

pub(super) async fn bind(d: &Deployment, example: ResponseExample) -> BindingTarget {
    let binding = Box::pin(d.bind(example.source, Some(MessageTrigger::Any))).await;
    d.configure_response(binding, example.contract, example.approval_policy)
        .await;
    binding
}

#[derive(sqlx::FromRow, Debug)]
pub(super) struct Final {
    pub run: Uuid,
    pub source: Uuid,
    pub thread: Uuid,
    pub state: String,
    pub message: Option<Uuid>,
    pub delivery: Option<Uuid>,
    pub output: Value,
}

pub(super) async fn finals(d: &Deployment) -> Vec<Final> {
    sqlx::query_as("SELECT owner.run_id AS run,context.message_id AS source,context.thread_id AS thread,decision.state,decision.message_id AS message,decision.delivery_id AS delivery,execution.committed_output AS output FROM channel_reply_runs AS owner JOIN channel_reply_contexts AS context ON context.company_id=owner.company_id AND context.intent_id=owner.intent_id JOIN channel_reply_decisions AS decision ON decision.company_id=owner.company_id AND decision.run_id=owner.run_id JOIN workflow_runs AS run ON run.company_id=owner.company_id AND run.id=owner.run_id JOIN workflow_executions AS execution ON execution.company_id=run.company_id AND execution.id=run.terminal_execution_id WHERE owner.company_id=$1 ORDER BY run.created_at")
        .bind(d.fixture.target.company.as_uuid()).fetch_all(d.p().pool()).await.unwrap()
}

pub(super) async fn delivered(d: &Deployment, expected: i64) -> bool {
    d.count("SELECT count(*) FROM message_deliveries WHERE company_id=$1 AND purpose='workflow_reply' AND status='delivered'").await == expected
}

pub(super) async fn responses_published(d: &Deployment, expected: usize) -> bool {
    let rows = finals(d).await;
    rows.len() == expected && rows.iter().all(|row| row.state == "published")
}

pub(super) fn model(example: &mut ResponseExample) {
    example.source["steps"]["start"] = json!({"type":"agent.run","with":{
        "agent":{"literal":"assistant"},"context":{"ref":"/input"},
        "output_schema":{"literal":example.source["output_schema"].clone()}},
        "routes":{"success":"$end"}});
}

pub(super) async fn call_child(d: &Deployment, source: Value) -> Value {
    let child = Box::pin(d.bind(source, None)).await;
    let state = d
        .p()
        .binding(d.fixture.target.company, child.binding)
        .await
        .unwrap()
        .unwrap();
    let definition = state.configuration.bundle().compiled().graph().definition();
    let mut parent = example(0).source;
    parent["steps"]["start"] = json!({"type":"workflow.call",
        "child_workflow_id":definition.workflow_id,"child_version_id":definition.version_id,
        "with":{"input":{"ref":"/input"}},"routes":{"success":"$end"}});
    parent
}

pub(super) async fn answer(
    d: &Deployment,
    key: &str,
    rfc: &str,
    text: &str,
) -> crate::use_cases::thread::InboundIngestResult {
    let mut payload = customer_email(d, key, None);
    payload.headers = Some(format!(
        "Message-ID: <{key}@partner.test>\nIn-Reply-To: {rfc}\nReferences: {rfc}\n"
    ));
    payload.text = Some(text.into());
    d.email(payload).await
}

pub(super) async fn prompt(d: &Deployment, ordinal: usize) -> (Uuid, String) {
    let prompts: Vec<(Uuid,String)> = sqlx::query_as("SELECT question.message_id,metadata.rfc_message_id FROM workflow_channel_questions AS question JOIN workflow_waits AS wait ON wait.company_id=question.company_id AND wait.id=question.wait_id JOIN email_message_metadata AS metadata ON metadata.company_id=question.company_id AND metadata.message_id=question.message_id WHERE question.company_id=$1 ORDER BY wait.created_at,question.wait_id")
        .bind(d.fixture.target.company.as_uuid()).fetch_all(d.p().pool()).await.unwrap();
    prompts[ordinal].clone()
}

pub(super) async fn waiting(d: &Deployment, expected: i64) -> bool {
    d.count("SELECT count(*) FROM workflow_channel_questions AS question JOIN workflow_waits AS wait ON wait.company_id=question.company_id AND wait.id=question.wait_id WHERE question.company_id=$1 AND wait.state='waiting'")
        .await
        == expected
}

pub(super) use crate::application::workflow::channel_reply::examples::{
    ResponseExample, response_examples,
};
