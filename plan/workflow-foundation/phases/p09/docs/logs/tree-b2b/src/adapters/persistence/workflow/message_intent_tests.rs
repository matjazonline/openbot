//! p08 D1.1/D1.2/D1.4 against PostgreSQL: which bindings an accepted message admits, under whose
//! authority, and how the intent queue survives reconfiguration, failure, competing workers,
//! tenant bursts and shutdown.
use super::thread_tests::inbound;
use super::*;
use crate::adapters::monitoring::in_memory_monitor::InMemoryMonitor;
use crate::application::use_cases::thread::ThreadPersistence;
use crate::application::workflow::message_admission::*;
use crate::domain::monitoring::MonitoringService;
use crate::domain::workflow::{MAX_MESSAGE_BINDINGS_PER_CHANNEL, MessageTrigger};
use crate::entities::message::CanonicalMessageId;
use crate::entities::value_objects::EmailAddress;
use futures::future::BoxFuture;
use std::collections::{BTreeMap, BTreeSet};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::Duration;
use tokio::sync::Notify;
use tokio_util::sync::CancellationToken;

#[path = "message_reply_tests.rs"]
mod reply_tests;

type Admission = WorkflowService<
    PostgresPersistence,
    PostgresPersistence,
    PostgresPersistence,
    PostgresPersistence,
    Preflight,
>;

fn admission(persistence: &PostgresPersistence) -> Admission {
    WorkflowService::new(
        persistence.clone(),
        persistence.clone(),
        persistence.clone(),
        persistence.clone(),
        Preflight,
    )
}

/// What a scripted admitter does with one call.
enum Script {
    Delegate,
    /// A transient failure before anything is written.
    Fail,
    /// The admission commits, then the worker crashes before it records the outcome.
    DelegateThenFail,
}

type Hook = Box<dyn Fn(usize, WorkflowBindingId) -> BoxFuture<'static, Script> + Send + Sync>;

/// The real admission service behind a per-call script, so a test decides exactly where the
/// interference lands.
struct Scripted {
    inner: Admission,
    calls: AtomicUsize,
    hook: Hook,
}

impl Scripted {
    fn new(persistence: &PostgresPersistence, hook: Hook) -> Self {
        Self {
            inner: admission(persistence),
            calls: AtomicUsize::new(0),
            hook,
        }
    }
}

#[async_trait]
impl WorkflowAdmitter for Scripted {
    async fn admit(&self, request: AdmitWorkflowRequest) -> AppResult<AdmissionResult> {
        let call = self.calls.fetch_add(1, Ordering::SeqCst);
        match (self.hook)(call, request.binding_id).await {
            Script::Delegate => self.inner.admit(request).await,
            Script::Fail => Err(AppError::Internal("scripted admission outage".into())),
            Script::DelegateThenFail => {
                self.inner.admit(request).await?;
                Err(AppError::Internal("scripted crash after admission".into()))
            }
        }
    }
}

/// How the accepted message reached the channel.
#[derive(Clone, Copy)]
struct Facts {
    role: &'static str,
    mentioned: bool,
    admissible: bool,
}

const TO: Facts = Facts {
    role: "to",
    mentioned: false,
    admissible: true,
};
const CC: Facts = Facts { role: "cc", ..TO };

/// A company, a channel with one customer thread, and a published workflow that accepts any
/// input, so every admission below is decided by bindings and authority alone.
struct Intents {
    fixture: Fixture,
    channel: Uuid,
    thread: Uuid,
    version: VersionId,
    messages: AtomicUsize,
}

impl Intents {
    async fn new() -> Self {
        Self::in_fixture(Fixture::new().await).await
    }

    async fn in_fixture(fixture: Fixture) -> Self {
        let channel = new_channel(&fixture, "runs").await;
        let thread = fixture
            .persistence
            .create_thread(
                channel,
                "Question",
                &[EmailAddress::from("customer@partner.test")],
            )
            .await
            .unwrap()
            .id;
        let service = fixture.service();
        service.save(fixture.save(None)).await.unwrap();
        let publication = fixture.publish();
        service.publish(publication.clone()).await.unwrap();
        Self {
            fixture,
            channel,
            thread,
            version: publication.version,
            messages: AtomicUsize::new(0),
        }
    }

    fn p(&self) -> &PostgresPersistence {
        &self.fixture.persistence
    }

    fn pool(&self) -> &sqlx::PgPool {
        self.p().pool()
    }

    fn owner(&self) -> WorkflowActor {
        self.fixture.target.actor
    }

    fn target(&self, actor: WorkflowActor, binding: WorkflowBindingId) -> BindingTarget {
        BindingTarget {
            company: self.fixture.target.company,
            actor,
            binding,
        }
    }

    fn bindings(
        &self,
    ) -> BindingService<Preflight, PostgresPersistence, PostgresPersistence, PostgresPersistence>
    {
        let p = self.p();
        BindingService::new(Preflight, p.clone(), p.clone(), p.clone())
    }

    fn configuration(
        &self,
        target: BindingTarget,
        expected: Option<BindingStateRevision>,
        run_as: Option<Uuid>,
        trigger: Option<MessageTrigger>,
    ) -> ConfigureBinding {
        ConfigureBinding {
            target,
            expected,
            association: RelatedAssociation::Channel(RelatedChannelId::new(self.channel)),
            version: self.version,
            params: json!({}),
            resources: BTreeMap::new(),
            run_as: run_as.map(|user| WorkflowActor::authenticated(user).unwrap()),
            message_trigger: trigger,
        }
    }

    /// An active binding on the channel, configured by the owner.
    async fn binding(&self, trigger: MessageTrigger, run_as: Option<Uuid>) -> WorkflowBindingId {
        let target = self.target(self.owner(), WorkflowBindingId::new(Uuid::new_v4()));
        let state = self
            .bindings()
            .configure(self.configuration(target, None, run_as, Some(trigger)))
            .await
            .unwrap();
        self.bindings()
            .activate(SetBindingActivity {
                target,
                expected: state.revision,
            })
            .await
            .unwrap();
        target.binding
    }

    async fn state(&self, binding: WorkflowBindingId) -> BindingState {
        self.p()
            .binding(self.fixture.target.company, binding)
            .await
            .unwrap()
            .unwrap()
    }

    async fn deactivate(&self, binding: WorkflowBindingId) {
        let state = self.state(binding).await;
        self.bindings()
            .deactivate(SetBindingActivity {
                target: self.target(self.owner(), binding),
                expected: state.revision,
            })
            .await
            .unwrap();
    }

    /// The owner reconfigures `binding`: new params, and the given trigger.
    async fn reconfigure(&self, binding: WorkflowBindingId, trigger: Option<MessageTrigger>) {
        let state = self.state(binding).await;
        let mut request = self.configuration(
            self.target(self.owner(), binding),
            Some(state.revision),
            None,
            trigger,
        );
        request.params = json!({"revised": true});
        self.bindings().configure(request).await.unwrap();
    }

    async fn message(&self) -> CanonicalMessageId {
        let n = self.messages.fetch_add(1, Ordering::SeqCst);
        self.p()
            .create_message(&inbound(
                self.thread,
                &format!("<question-{n}@partner.test>"),
                &[],
                "Could you help?",
            ))
            .await
            .unwrap()
            .canonical_id
    }

    /// An accepted message's intent on the channel, as the inbound commit writes it.
    async fn intent(&self, facts: Facts) -> Uuid {
        self.intent_with(facts, "deliver").await
    }

    /// `intent` under a delivery policy, as a simulation's in-app test mode writes it.
    async fn intent_with(&self, facts: Facts, policy: &str) -> Uuid {
        let message = self.message().await;
        let id = Uuid::new_v4();
        sqlx::query(
            "INSERT INTO workflow_message_intents (company_id, id, message_id, channel_id, \
               thread_id, role, mentioned, admissible, origin, responder_principal_id, hop_count, \
               trace_channels, delivery_policy, correlation_id) \
             SELECT $1, $2, message.id, $3, $4, $5, $6, $7, 'external', message.author_principal_id, \
               0, '{}', $9, $2 \
             FROM messages AS message WHERE message.company_id = $1 AND message.id = $8",
        )
        .bind(self.fixture.target.company.as_uuid())
        .bind(id)
        .bind(self.channel)
        .bind(self.thread)
        .bind(facts.role)
        .bind(facts.mentioned)
        .bind(facts.admissible)
        .bind(message.as_uuid())
        .bind(policy)
        .execute(self.pool())
        .await
        .unwrap();
        id
    }

    /// Make a deferred intent due again without waiting for its backoff.
    async fn make_due(&self, intent: Uuid) {
        sqlx::query(
            "UPDATE workflow_message_intents SET available_at = clock_timestamp() WHERE id = $1",
        )
        .bind(intent)
        .execute(self.pool())
        .await
        .unwrap();
    }

    /// Let a claim's lease lapse, as a crashed worker's would.
    async fn expire(&self, intent: Uuid) {
        sqlx::query(
            "UPDATE workflow_message_intents SET lease_expires_at = clock_timestamp() - interval '1 second' \
             WHERE id = $1",
        )
        .bind(intent)
        .execute(self.pool())
        .await
        .unwrap();
    }

    fn admissions<A: WorkflowAdmitter>(
        &self,
        admitter: A,
    ) -> MessageAdmissions<PostgresPersistence, A, PostgresPersistence> {
        MessageAdmissions::new(self.p().clone(), admitter, self.p().clone())
    }

    /// One worker iteration: claim a page and process every intent in it.
    async fn drain<A: WorkflowAdmitter>(
        &self,
        admissions: &MessageAdmissions<PostgresPersistence, A, PostgresPersistence>,
    ) -> usize {
        let claimed = admissions
            .claim(MessageIntentWorkerId(Uuid::new_v4()), MESSAGE_INTENT_PAGE)
            .await
            .unwrap();
        let count = claimed.len();
        for intent in claimed {
            admissions.process(intent).await.unwrap();
        }
        count
    }

    /// `(state, outcome, attempts)` of an intent.
    async fn intent_state(&self, intent: Uuid) -> (String, Option<String>, i16) {
        sqlx::query_as(
            "SELECT state, outcome, attempts FROM workflow_message_intents WHERE id = $1",
        )
        .bind(intent)
        .fetch_one(self.pool())
        .await
        .unwrap()
    }

    /// Every binding outcome of an intent, as `binding -> (outcome, reason)`.
    async fn outcomes(&self, intent: Uuid) -> BTreeMap<Uuid, (String, Option<String>)> {
        let rows: Vec<(Uuid, String, Option<String>)> = sqlx::query_as(
            "SELECT binding_id, outcome, reason FROM workflow_message_intent_bindings \
             WHERE intent_id = $1",
        )
        .bind(intent)
        .fetch_all(self.pool())
        .await
        .unwrap();
        rows.into_iter()
            .map(|(binding, outcome, reason)| (binding, (outcome, reason)))
            .collect()
    }

    /// The bindings an intent admitted a run for.
    async fn admitted(&self, intent: Uuid) -> BTreeSet<Uuid> {
        self.outcomes(intent)
            .await
            .into_iter()
            .filter(|(_, (outcome, _))| outcome == "admitted")
            .map(|(binding, _)| binding)
            .collect()
    }

    /// The runs a binding has, by id.
    async fn runs(&self, binding: WorkflowBindingId) -> Vec<Uuid> {
        sqlx::query_scalar("SELECT id FROM workflow_runs WHERE binding_id = $1 ORDER BY id")
            .bind(binding.as_uuid())
            .fetch_all(self.pool())
            .await
            .unwrap()
    }
}

fn ids(bindings: &[WorkflowBindingId]) -> BTreeSet<Uuid> {
    bindings.iter().map(|binding| binding.as_uuid()).collect()
}

/// D1.2/D1.4.4: each rule admits by role and mention, every eligible binding gets a run of its
/// own, and a quiet message admits nothing. Every run names the message, its thread and the
/// binding's run-as.
#[tokio::test]
async fn message_intent_rules_admit_each_eligible_binding_independently() {
    let s = Intents::new().await;
    let direct = s.binding(MessageTrigger::Direct, None).await;
    let any = s.binding(MessageTrigger::Any, None).await;
    let mentioned = s.binding(MessageTrigger::Mentioned, None).await;
    let reply = s.binding(MessageTrigger::MentionedOrReply, None).await;
    let to = s.intent(TO).await;
    let cc = s.intent(CC).await;
    let named = s
        .intent(Facts {
            mentioned: true,
            ..CC
        })
        .await;
    let quiet = s
        .intent(Facts {
            admissible: false,
            ..TO
        })
        .await;

    assert_eq!(s.drain(&s.admissions(admission(s.p()))).await, 4);

    assert_eq!(s.admitted(to).await, ids(&[direct, any]));
    assert_eq!(s.admitted(cc).await, ids(&[any]));
    assert_eq!(
        s.admitted(named).await,
        ids(&[direct, any, mentioned, reply])
    );
    assert!(s.outcomes(quiet).await.is_empty());
    for (intent, outcome) in [(to, "admitted"), (cc, "admitted"), (quiet, "none")] {
        let (state, recorded, attempts) = s.intent_state(intent).await;
        assert_eq!(
            (state.as_str(), recorded.as_deref(), attempts),
            ("done", Some(outcome), 1)
        );
    }
    // Two bindings, two independent runs of the same message.
    let runs: Vec<(Uuid, Uuid, Uuid, Uuid, Uuid, Value)> = sqlx::query_as(
        "SELECT run.binding_id, run.trigger_id, run.channel_id, run.thread_id, run.actor_id, run.input \
         FROM workflow_runs AS run WHERE run.trigger_id = $1 ORDER BY run.binding_id",
    )
    .bind(to)
    .fetch_all(s.pool())
    .await
    .unwrap();
    assert_eq!(
        runs.iter().map(|run| run.0).collect::<BTreeSet<_>>(),
        ids(&[direct, any])
    );
    let message: Uuid =
        sqlx::query_scalar("SELECT message_id FROM workflow_message_intents WHERE id = $1")
            .bind(to)
            .fetch_one(s.pool())
            .await
            .unwrap();
    for (_, trigger, channel, thread, actor, input) in &runs {
        assert_eq!(
            (*trigger, *channel, *thread, *actor),
            (to, s.channel, s.thread, s.owner().user_id())
        );
        assert_eq!(input["source_message"], message.to_string());
        assert_eq!(input["message"], "Could you help?");
    }
}

/// D1.1/D1.4.6: a run-as who is no longer a member refuses that binding's admission visibly
/// (outcome row, reason, metric) while its sibling is admitted; removing that member is not
/// blocked by the revisions that name them.
#[tokio::test]
async fn message_intent_refused_run_as_is_visible_and_spares_its_sibling() {
    let s = Intents::new().await;
    let admin = member(&s.fixture, "admin").await;
    let removed = s.binding(MessageTrigger::Any, Some(admin)).await;
    let sibling = s.binding(MessageTrigger::Any, None).await;
    sqlx::query("DELETE FROM company_members WHERE company_id = $1 AND user_id = $2")
        .bind(s.fixture.target.company.as_uuid())
        .bind(admin)
        .execute(s.pool())
        .await
        .expect("removing a member who is a binding's run-as is never blocked");
    let intent = s.intent(TO).await;
    let monitor = Arc::new(InMemoryMonitor::new());
    let admissions = s
        .admissions(admission(s.p()))
        .with_monitoring(monitor.clone());

    s.drain(&admissions).await;

    let outcomes = s.outcomes(intent).await;
    assert_eq!(
        outcomes[&removed.as_uuid()],
        ("refused".to_string(), Some("unavailable".to_string()))
    );
    assert_eq!(outcomes[&sibling.as_uuid()].0, "admitted");
    assert!(s.runs(removed).await.is_empty());
    assert_eq!(s.runs(sibling).await.len(), 1);
    assert_eq!(s.intent_state(intent).await.1.as_deref(), Some("admitted"));
    assert_eq!(
        monitor.get_stats_json()["custom_counters"]["workflow_message_admission_refusals_total{reason=unavailable}"],
        1
    );
}

/// D1.2 activation time and D1.4.4: a binding activated after the message was accepted, and one
/// deactivated before it was processed, admit nothing.
#[tokio::test]
async fn message_intent_ignores_bindings_activated_later_or_deactivated() {
    let s = Intents::new().await;
    let deactivated = s.binding(MessageTrigger::Any, None).await;
    let intent = s.intent(TO).await;
    let later = s.binding(MessageTrigger::Any, None).await;
    s.deactivate(deactivated).await;

    s.drain(&s.admissions(admission(s.p()))).await;

    assert!(s.outcomes(intent).await.is_empty());
    assert!(s.runs(deactivated).await.is_empty());
    assert!(s.runs(later).await.is_empty());
    assert_eq!(s.intent_state(intent).await.1.as_deref(), Some("none"));
}

/// D1.4.5 `Stale`: a binding reconfigured while its admission was in flight is re-read and
/// admitted under its new revision; one whose new revision drops the trigger no longer wants the
/// message and records nothing.
#[tokio::test]
async fn message_intent_reconfigured_binding_is_re_read_before_admission() {
    let s = Arc::new(Intents::new().await);
    let revised = s.binding(MessageTrigger::Any, None).await;
    let dropped = s.binding(MessageTrigger::Any, None).await;
    let intent = s.intent(TO).await;
    let seen = Arc::new(std::sync::Mutex::new(Vec::new()));
    let hook: Hook = {
        let s = s.clone();
        let seen = seen.clone();
        Box::new(move |_, binding| {
            let s = s.clone();
            let mut calls = seen.lock().unwrap();
            let first = !calls.contains(&binding);
            calls.push(binding);
            Box::pin(async move {
                if first {
                    // The owner saves a new revision between the eligibility read and admission.
                    let trigger = (binding == revised).then_some(MessageTrigger::Any);
                    s.reconfigure(binding, trigger).await;
                }
                Script::Delegate
            })
        })
    };

    s.drain(&s.admissions(Scripted::new(s.p(), hook))).await;

    // The stale attempt, then the retry under the re-read revision; the dropped binding is never
    // retried because its re-read is no longer eligible.
    let calls = seen.lock().unwrap().clone();
    let count = |binding| calls.iter().filter(|call| **call == binding).count();
    assert_eq!((count(revised), count(dropped)), (2, 1));
    let outcomes = s.outcomes(intent).await;
    assert_eq!(outcomes.len(), 1, "{outcomes:?}");
    assert_eq!(outcomes[&revised.as_uuid()].0, "admitted");
    let (run_revision, current): (i64, i64) = sqlx::query_as(
        "SELECT run.binding_revision, binding.configuration_revision FROM workflow_runs AS run \
         JOIN workflow_bindings AS binding ON binding.company_id = run.company_id AND binding.id = run.binding_id \
         WHERE run.binding_id = $1",
    )
    .bind(revised.as_uuid())
    .fetch_one(s.pool())
    .await
    .unwrap();
    assert_eq!((run_revision, current), (2, 2));
    assert!(s.runs(dropped).await.is_empty());
    assert_eq!(s.intent_state(intent).await.1.as_deref(), Some("admitted"));
}

/// D1.4.5 transient: one binding's outage defers the intent without blocking its sibling, the
/// retry admits only the binding still owed, and the last attempt records the failure.
#[tokio::test]
async fn message_intent_transient_failure_retries_only_the_failed_binding() {
    let s = Intents::new().await;
    let flaky = s.binding(MessageTrigger::Any, None).await;
    let steady = s.binding(MessageTrigger::Any, None).await;
    let failing = || {
        Scripted::new(
            s.p(),
            Box::new(move |_, binding| {
                Box::pin(async move {
                    if binding == flaky {
                        Script::Fail
                    } else {
                        Script::Delegate
                    }
                })
            }),
        )
    };
    let intent = s.intent(TO).await;

    s.drain(&s.admissions(failing())).await;
    let outcomes = s.outcomes(intent).await;
    assert_eq!(outcomes.len(), 1);
    assert_eq!(outcomes[&steady.as_uuid()].0, "admitted");
    assert_eq!(
        s.intent_state(intent).await,
        ("pending".to_string(), None, 1)
    );
    assert_eq!(
        s.drain(&s.admissions(admission(s.p()))).await,
        0,
        "the backoff keeps a deferred intent out of the next page"
    );
    s.make_due(intent).await;
    s.drain(&s.admissions(admission(s.p()))).await;
    assert_eq!(s.admitted(intent).await, ids(&[flaky, steady]));
    assert_eq!(
        s.runs(steady).await.len(),
        1,
        "the sibling is not admitted twice"
    );
    assert_eq!(
        s.intent_state(intent).await,
        ("done".to_string(), Some("admitted".to_string()), 2)
    );

    // On the last attempt every binding still owed records its failure, and the intent ends.
    let last = s.intent(TO).await;
    sqlx::query("UPDATE workflow_message_intents SET attempts = $2 - 1 WHERE id = $1")
        .bind(last)
        .bind(MAX_INTENT_ATTEMPTS)
        .execute(s.pool())
        .await
        .unwrap();
    s.drain(&s.admissions(failing())).await;
    let outcomes = s.outcomes(last).await;
    assert_eq!(
        outcomes[&flaky.as_uuid()],
        (
            "failed".to_string(),
            Some("transient_exhausted".to_string())
        )
    );
    assert_eq!(outcomes[&steady.as_uuid()].0, "admitted");
    assert_eq!(
        s.intent_state(last).await,
        (
            "done".to_string(),
            Some("admitted".to_string()),
            MAX_INTENT_ATTEMPTS
        )
    );
}

/// D1.4.1 fencing with competing claimants: one claim wins; after its lease lapses a second
/// worker takes over under a new generation; every write of the stale claim changes nothing; both
/// processing the intent at once still admit exactly one run.
#[tokio::test]
async fn message_intent_competing_claimants_admit_one_run_and_fence_the_stale_worker() {
    let s = Intents::new().await;
    let binding = s.binding(MessageTrigger::Any, None).await;
    let intent = s.intent(TO).await;
    let p = s.p();
    let (left, right) = tokio::join!(
        p.claim_intents(MessageIntentWorkerId(Uuid::new_v4()), MESSAGE_INTENT_PAGE),
        p.claim_intents(MessageIntentWorkerId(Uuid::new_v4()), MESSAGE_INTENT_PAGE)
    );
    let mut won: Vec<_> = left.unwrap().into_iter().chain(right.unwrap()).collect();
    assert_eq!(won.len(), 1, "two claimants never share an intent");
    let stale = won.pop().unwrap();

    s.expire(intent).await;
    let current = p
        .claim_intents(MessageIntentWorkerId(Uuid::new_v4()), MESSAGE_INTENT_PAGE)
        .await
        .unwrap()
        .pop()
        .expect("an expired claim is reclaimable");
    assert_ne!(current.claim.generation, stale.claim.generation);
    assert_eq!(current.attempts, 2, "the lapsed lease spent its attempt");

    let candidate = p.candidate_bindings(&stale).await.unwrap().pop().unwrap();
    assert!(
        !p.record_binding_outcome(
            stale.claim,
            &candidate,
            BindingOutcome::Refused("unavailable")
        )
        .await
        .unwrap()
    );
    assert!(!p.finish_intent(stale.claim, None).await.unwrap());
    assert!(
        !p.defer_intent(stale.claim, IntentDeferral::Transient)
            .await
            .unwrap()
    );
    assert!(
        !p.defer_intent(stale.claim, IntentDeferral::NotYet)
            .await
            .unwrap()
    );
    assert!(s.outcomes(intent).await.is_empty());

    let admissions = s.admissions(admission(p));
    let (a, b) = tokio::join!(admissions.process(stale), admissions.process(current));
    a.unwrap();
    b.unwrap();
    assert_eq!(s.runs(binding).await.len(), 1);
    assert_eq!(s.admitted(intent).await, ids(&[binding]));
    assert_eq!(
        s.intent_state(intent).await,
        ("done".to_string(), Some("admitted".to_string()), 2)
    );
}

/// Failure mode "crash after admission, before the outcome": the retry replays the same run.
#[tokio::test]
async fn message_intent_crash_after_admission_replays_its_run() {
    let s = Intents::new().await;
    let binding = s.binding(MessageTrigger::Any, None).await;
    let intent = s.intent(TO).await;
    let crashing = Scripted::new(
        s.p(),
        Box::new(|_, _| Box::pin(async { Script::DelegateThenFail })),
    );

    s.drain(&s.admissions(crashing)).await;
    let first = s.runs(binding).await;
    assert_eq!(first.len(), 1, "the admission committed");
    assert!(s.outcomes(intent).await.is_empty(), "its outcome did not");

    s.make_due(intent).await;
    s.drain(&s.admissions(admission(s.p()))).await;
    assert_eq!(
        s.runs(binding).await,
        first,
        "the retry replays the same run"
    );
    let run: Uuid = sqlx::query_scalar(
        "SELECT run_id FROM workflow_message_intent_bindings WHERE intent_id = $1",
    )
    .bind(intent)
    .fetch_one(s.pool())
    .await
    .unwrap();
    assert_eq!(run, first[0]);
}

/// `src/application/AGENTS.md` poison batch: a page of intents that all fail is deferred out of
/// the claimable set, so the next iteration without any time passing claims none of them.
#[tokio::test]
async fn message_intent_poison_batch_is_not_reclaimed() {
    let s = Intents::new().await;
    s.binding(MessageTrigger::Any, None).await;
    let page = 3;
    let mut poisoned = BTreeSet::new();
    for _ in 0..page {
        poisoned.insert(s.intent(TO).await);
    }
    let admissions = s.admissions(Scripted::new(
        s.p(),
        Box::new(|_, _| Box::pin(async { Script::Fail })),
    ));
    let worker = MessageIntentWorkerId(Uuid::new_v4());
    for iteration in 0..2 {
        let claimed = admissions
            .queue()
            .claim_intents(worker, page as u16)
            .await
            .unwrap();
        if iteration == 1 {
            assert!(claimed.is_empty(), "the failed page was reclaimed");
            break;
        }
        assert_eq!(
            claimed
                .iter()
                .map(|intent| intent.claim.intent)
                .collect::<BTreeSet<_>>(),
            poisoned
        );
        for intent in claimed {
            admissions.process(intent).await.unwrap();
        }
    }
    for intent in poisoned {
        assert_eq!(
            s.intent_state(intent).await,
            ("pending".to_string(), None, 1)
        );
    }
}

/// D1.4.1 fairness: a tenant with a hundred pending intents takes at most four places in a page,
/// so another tenant's single intent is claimed in the first page.
#[tokio::test]
async fn message_intent_claims_are_fair_across_tenants() {
    let db = Arc::new(own_database().await.expect("a database of its own"));
    let busy = Intents::in_fixture(Fixture::in_database(db.clone(), "-busy").await).await;
    let quiet = Intents::in_fixture(Fixture::in_database(db.clone(), "-quiet").await).await;
    for _ in 0..100 {
        busy.intent(TO).await;
    }
    let lone = quiet.intent(TO).await;

    let page = busy
        .p()
        .claim_intents(MessageIntentWorkerId(Uuid::new_v4()), MESSAGE_INTENT_PAGE)
        .await
        .unwrap();

    let from_busy = page
        .iter()
        .filter(|intent| intent.claim.company == busy.fixture.target.company)
        .count();
    assert_eq!(from_busy, usize::from(MESSAGE_INTENTS_PER_COMPANY));
    assert!(page.iter().any(|intent| intent.claim.intent == lone));
}

/// Own background work through shutdown: the worker stops between iterations and mid-intent;
/// an interrupted intent keeps a short lease whose expiry spends an attempt and is then
/// processed once, with no run admitted by the interrupted pass.
#[tokio::test]
async fn message_intent_worker_stops_between_and_during_processing() {
    let s = Intents::new().await;
    let binding = s.binding(MessageTrigger::Any, None).await;
    let wakeup = Notify::new();

    // Between iterations: nothing to do, then shutdown.
    let idle = MessageAdmissionWorker {
        admissions: s.admissions(admission(s.p())),
        worker: MessageIntentWorkerId(Uuid::new_v4()),
        poll: Duration::from_millis(20),
    };
    let shutdown = CancellationToken::new();
    let ((), stopped) = tokio::join!(
        async {
            tokio::time::sleep(Duration::from_millis(100)).await;
            shutdown.cancel();
        },
        tokio::time::timeout(Duration::from_secs(5), idle.run(&wakeup, &shutdown))
    );
    stopped.expect("an idle worker stops on shutdown");

    // During processing: the admission is in flight when shutdown arrives.
    let intent = s.intent(TO).await;
    let entered = Arc::new(Notify::new());
    let held = Arc::new(Notify::new());
    let busy = MessageAdmissionWorker {
        admissions: s.admissions(Scripted::new(s.p(), {
            let entered = entered.clone();
            let held = held.clone();
            Box::new(move |_, _| {
                let entered = entered.clone();
                let held = held.clone();
                Box::pin(async move {
                    entered.notify_one();
                    held.notified().await;
                    Script::Delegate
                })
            })
        })),
        worker: MessageIntentWorkerId(Uuid::new_v4()),
        poll: Duration::from_millis(20),
    };
    let shutdown = CancellationToken::new();
    let ((), stopped) = tokio::join!(
        async {
            entered.notified().await;
            shutdown.cancel();
        },
        tokio::time::timeout(Duration::from_secs(5), busy.run(&wakeup, &shutdown))
    );
    stopped.expect("a busy worker stops on shutdown");
    assert_eq!(
        s.intent_state(intent).await,
        ("claimed".to_string(), None, 1)
    );
    assert!(s.runs(binding).await.is_empty(), "no orphaned run");

    s.expire(intent).await;
    s.drain(&s.admissions(admission(s.p()))).await;
    assert_eq!(s.runs(binding).await.len(), 1);
    assert_eq!(
        s.intent_state(intent).await,
        ("done".to_string(), Some("admitted".to_string()), 2)
    );
}

// ---------------------------------------------------------------------------------------------
// Binding run-as and message trigger (D1.1, D1.2)
// ---------------------------------------------------------------------------------------------

async fn run_as(s: &Intents, binding: WorkflowBindingId) -> Uuid {
    sqlx::query_scalar(
        "SELECT configuration.run_as_user_id FROM workflow_bindings AS binding \
         JOIN workflow_binding_revisions AS configuration ON configuration.company_id = binding.company_id \
           AND configuration.binding_id = binding.id AND configuration.revision = binding.configuration_revision \
         WHERE binding.id = $1",
    )
    .bind(binding.as_uuid())
    .fetch_one(s.pool())
    .await
    .unwrap()
}

/// Refused as a request, not failed as a system: a validation diagnostic or a client error.
fn refused<T>(result: LifecycleResult<T>) -> bool {
    matches!(
        result,
        Err(LifecycleError::Validation(_)
            | LifecycleError::Application(
                AppError::BadRequest(_) | AppError::NotFound(_) | AppError::Conflict(_)
            ))
    )
}

/// D1.1 who may choose: the owner names any current owner/admin; an admin names only
/// themselves; leaving the run-as alone on reconfigure keeps it; a plain member is never one.
#[tokio::test]
async fn binding_run_as_follows_the_owner_and_admin_self_rule() {
    let s = Intents::new().await;
    let admin = member(&s.fixture, "admin").await;
    let colleague = member(&s.fixture, "admin").await;
    let plain = member(&s.fixture, "member").await;
    let owner = s.owner().user_id();
    let as_admin = WorkflowActor::authenticated(admin).unwrap();
    let new = || WorkflowBindingId::new(Uuid::new_v4());
    let bindings = s.bindings();

    // Created with no run-as: the configuring caller.
    let mine = s.target(as_admin, new());
    bindings
        .configure(s.configuration(mine, None, None, Some(MessageTrigger::Any)))
        .await
        .unwrap();
    assert_eq!(run_as(&s, mine.binding).await, admin);
    // An admin naming themselves is no choice at all; naming anyone else is refused.
    let own = s.target(as_admin, new());
    bindings
        .configure(s.configuration(own, None, Some(admin), None))
        .await
        .unwrap();
    for other in [colleague, owner] {
        assert!(refused(
            bindings
                .configure(s.configuration(s.target(as_admin, new()), None, Some(other), None))
                .await
        ));
    }
    // The owner names an admin, never a plain member.
    let chosen = s.target(s.owner(), new());
    let state = bindings
        .configure(s.configuration(chosen, None, Some(colleague), None))
        .await
        .unwrap();
    assert_eq!(run_as(&s, chosen.binding).await, colleague);
    assert!(refused(
        bindings
            .configure(s.configuration(s.target(s.owner(), new()), None, Some(plain), None))
            .await
    ));
    // An admin editing the owner's binding keeps the owner-chosen run-as by leaving it alone,
    // and cannot replace it with someone else's.
    let edited = bindings
        .configure(s.configuration(
            s.target(as_admin, chosen.binding),
            Some(state.revision),
            None,
            Some(MessageTrigger::Direct),
        ))
        .await
        .unwrap();
    assert_eq!(run_as(&s, chosen.binding).await, colleague);
    assert!(refused(
        bindings
            .configure(s.configuration(
                s.target(as_admin, chosen.binding),
                Some(edited.revision),
                Some(owner),
                None,
            ))
            .await
    ));
}

/// D1.2: a message trigger needs a channel or thread association and an input schema that
/// accepts the standard message input.
#[tokio::test]
async fn binding_message_trigger_needs_a_channel_and_a_compatible_input() {
    let s = Intents::new().await;
    let mut company = s.configuration(
        s.target(s.owner(), WorkflowBindingId::new(Uuid::new_v4())),
        None,
        None,
        Some(MessageTrigger::Any),
    );
    company.association = RelatedAssociation::Company;
    assert!(refused(s.bindings().configure(company).await));

    let mut source: Value =
        serde_json::from_str(&registry::example("data.map").unwrap().source).unwrap();
    source["input_schema"] = json!({"type":"object","properties":{"message":{"type":"integer"}}});
    let strict = s
        .fixture
        .publish_workflow(WorkflowId::new(Uuid::new_v4()), source)
        .await;
    let mut incompatible = s.configuration(
        s.target(s.owner(), WorkflowBindingId::new(Uuid::new_v4())),
        None,
        None,
        Some(MessageTrigger::Any),
    );
    incompatible.version = strict;
    assert!(refused(s.bindings().configure(incompatible).await));
}

/// D1.2 bound: at most eight active message-triggered bindings per channel, enforced on
/// activation and on adding a trigger to an active binding; a freed place can be taken again.
#[tokio::test]
async fn binding_message_trigger_cap_is_enforced_on_activation_and_reconfiguration() {
    let s = Intents::new().await;
    let mut active = Vec::new();
    for _ in 0..MAX_MESSAGE_BINDINGS_PER_CHANNEL {
        active.push(s.binding(MessageTrigger::Any, None).await);
    }
    let bindings = s.bindings();

    let ninth = s.target(s.owner(), WorkflowBindingId::new(Uuid::new_v4()));
    let configured = bindings
        .configure(s.configuration(ninth, None, None, Some(MessageTrigger::Any)))
        .await
        .unwrap();
    assert!(refused(
        bindings
            .activate(SetBindingActivity {
                target: ninth,
                expected: configured.revision,
            })
            .await
    ));

    let plain = s.target(s.owner(), WorkflowBindingId::new(Uuid::new_v4()));
    let state = bindings
        .configure(s.configuration(plain, None, None, None))
        .await
        .unwrap();
    let state = bindings
        .activate(SetBindingActivity {
            target: plain,
            expected: state.revision,
        })
        .await
        .expect("a binding without a message trigger takes no place");
    assert!(refused(
        bindings
            .configure(s.configuration(
                plain,
                Some(state.revision),
                None,
                Some(MessageTrigger::Any)
            ))
            .await
    ));

    s.deactivate(active[0]).await;
    let configured = s.state(ninth.binding).await;
    bindings
        .activate(SetBindingActivity {
            target: ninth,
            expected: configured.revision,
        })
        .await
        .expect("a freed place can be taken");
}

// ---------------------------------------------------------------------------------------------
// Terminal failure, tenancy and delivery policy (WF-P08-F023, F025)
// ---------------------------------------------------------------------------------------------

/// WF-P08-F023: a claim whose lease lapses on its last attempt is failed by the next claim,
/// logged and counted apart from binding refusals, and never reclaimed.
#[tokio::test]
async fn message_intent_lease_lapsing_on_the_last_attempt_fails_it_observably() {
    let s = Intents::new().await;
    let binding = s.binding(MessageTrigger::Any, None).await;
    let intent = s.intent(TO).await;
    sqlx::query("UPDATE workflow_message_intents SET attempts = $2 - 1 WHERE id = $1")
        .bind(intent)
        .bind(MAX_INTENT_ATTEMPTS)
        .execute(s.pool())
        .await
        .unwrap();
    let monitor = Arc::new(InMemoryMonitor::new());
    let admissions = s
        .admissions(admission(s.p()))
        .with_monitoring(monitor.clone());
    let worker = MessageIntentWorkerId(Uuid::new_v4());
    let claimed = admissions.claim(worker, MESSAGE_INTENT_PAGE).await.unwrap();
    assert_eq!(claimed.len(), 1, "the last attempt is claimed");
    // Its worker dies with it.
    s.expire(intent).await;

    assert!(
        admissions
            .claim(worker, MESSAGE_INTENT_PAGE)
            .await
            .unwrap()
            .is_empty()
    );
    assert_eq!(
        s.intent_state(intent).await,
        ("failed".to_string(), None, MAX_INTENT_ATTEMPTS)
    );
    assert_eq!(
        monitor.get_stats_json()["custom_counters"]["workflow_message_intents_failed_total{cause=lease_expired}"],
        1
    );
    assert!(s.runs(binding).await.is_empty());
}

fn foreign_key_violation(result: Result<sqlx::postgres::PgQueryResult, sqlx::Error>) -> bool {
    result
        .err()
        .and_then(|error| {
            error
                .as_database_error()
                .and_then(|db| db.code().map(|code| code.into_owned()))
        })
        .as_deref()
        == Some("23503")
}

/// D1.3 r2 (F008.4, WF-P08-F025.1): an intent and its binding outcomes name only their own
/// company's binding, runs, answered message and responder.
#[tokio::test]
async fn message_intent_rows_cannot_name_another_companys_records() {
    let db = Arc::new(own_database().await.expect("a database of its own"));
    let own = Intents::in_fixture(Fixture::in_database(db.clone(), "-own").await).await;
    let foreign = Intents::in_fixture(Fixture::in_database(db.clone(), "-foreign").await).await;
    let foreign_binding = foreign.binding(MessageTrigger::Any, None).await;
    let foreign_intent = foreign.intent(TO).await;
    foreign
        .drain(&foreign.admissions(admission(foreign.p())))
        .await;
    let foreign_run = foreign.runs(foreign_binding).await[0];
    let (foreign_message, foreign_principal): (Uuid, Uuid) = sqlx::query_as(
        "SELECT message_id, responder_principal_id FROM workflow_message_intents WHERE id = $1",
    )
    .bind(foreign_intent)
    .fetch_one(own.pool())
    .await
    .unwrap();
    let binding = own.binding(MessageTrigger::Any, None).await;
    let intent = own.intent(TO).await;
    let company = own.fixture.target.company.as_uuid();
    let outcome = |binding: Uuid, run: Option<Uuid>| {
        sqlx::query(
            "INSERT INTO workflow_message_intent_bindings (company_id, intent_id, binding_id, \
               binding_revision, outcome, run_id, reason) \
             VALUES ($1, $2, $3, 1, CASE WHEN $4::uuid IS NULL THEN 'refused' ELSE 'admitted' END, \
               $4, CASE WHEN $4::uuid IS NULL THEN 'unavailable' END)",
        )
        .bind(company)
        .bind(intent)
        .bind(binding)
        .bind(run)
    };
    assert!(foreign_key_violation(
        outcome(foreign_binding.as_uuid(), None)
            .execute(own.pool())
            .await
    ));
    assert!(foreign_key_violation(
        outcome(binding.as_uuid(), Some(foreign_run))
            .execute(own.pool())
            .await
    ));
    let update = |set: &'static str, value: Uuid| {
        let pool = own.pool().clone();
        async move {
            sqlx::query(&format!(
                "UPDATE workflow_message_intents SET {set} WHERE company_id = $1 AND id = $2"
            ))
            .bind(company)
            .bind(intent)
            .bind(value)
            .execute(&pool)
            .await
        }
    };
    assert!(foreign_key_violation(
        update(
            "state = 'done', outcome = 'matched', matched_run_id = $3",
            foreign_run
        )
        .await
    ));
    assert!(foreign_key_violation(
        update("reply_to_message_id = $3", foreign_message).await
    ));
    assert!(foreign_key_violation(
        update("responder_principal_id = $3", foreign_principal).await
    ));
}

/// D1.7 (WF-P08-F025.2): the delivery policy is part of admission replay equivalence.
#[tokio::test]
async fn message_intent_admission_replay_with_another_delivery_policy_conflicts() {
    use crate::application::workflow::DeliveryPolicy;
    let s = Intents::new().await;
    let binding = s.binding(MessageTrigger::Any, None).await;
    let company = s.fixture.target.company;
    let trigger = TriggerId::new(Uuid::new_v4());
    let correlation = crate::entities::correlation::CorrelationId::new();
    let request = |policy| AdmitWorkflowRequest {
        company_id: company,
        actor: s.owner(),
        association: RelatedAssociation::Channel(RelatedChannelId::new(s.channel)),
        trigger: TriggerRef::new(company, trigger, TriggerSource::Manual).unwrap(),
        correlation_id: correlation,
        binding_id: binding,
        idempotency_key: IdempotencyKey::parse("policy-replay").unwrap(),
        input: json!({}),
        rerun_of: None,
        delivery_policy: policy,
        expected_binding_revision: None,
    };
    let service = admission(s.p());
    let AdmissionResult::Created(run) = service
        .admit(request(DeliveryPolicy::Deliver))
        .await
        .unwrap()
    else {
        panic!("the first start creates its run");
    };
    assert_eq!(
        service
            .admit(request(DeliveryPolicy::Deliver))
            .await
            .unwrap(),
        AdmissionResult::Replayed(run)
    );
    assert_eq!(
        service
            .admit(request(DeliveryPolicy::InAppOnly))
            .await
            .unwrap(),
        AdmissionResult::Conflict
    );
}

/// D1.7 (WF-P08-F025.3): a simulation's in-app intent admits an in-app run, so its tree never
/// mails anyone.
#[tokio::test]
async fn message_intent_in_app_policy_reaches_the_admitted_run() {
    let s = Intents::new().await;
    let binding = s.binding(MessageTrigger::Any, None).await;
    let intent = s.intent_with(TO, "in_app_only").await;

    s.drain(&s.admissions(admission(s.p()))).await;

    let policy: String = sqlx::query_scalar(
        "SELECT delivery_policy FROM workflow_runs WHERE binding_id = $1 AND trigger_id = $2",
    )
    .bind(binding.as_uuid())
    .bind(intent)
    .fetch_one(s.pool())
    .await
    .unwrap();
    assert_eq!(policy, "in_app_only");
}

/// p08 D4.2: a binding's refusals in the last 7 days are one team-work row with their count and
/// latest reason, on the binding's channel. An admitted sibling has none, nobody's own or
/// unassigned work holds it, and a viewer who cannot read the channel sees none.
#[tokio::test]
async fn attention_groups_a_bindings_refusals_into_one_team_work_row() {
    use crate::application::attention::AttentionPersistence;
    use crate::entities::attention::{AttentionQuery, AttentionSourceKind, AttentionView};
    use crate::entities::transport::PrincipalId;

    let s = Intents::new().await;
    let admin = member(&s.fixture, "admin").await;
    let removed = s.binding(MessageTrigger::Any, Some(admin)).await;
    let sibling = s.binding(MessageTrigger::Any, None).await;
    sqlx::query("DELETE FROM company_members WHERE company_id = $1 AND user_id = $2")
        .bind(s.fixture.target.company.as_uuid())
        .bind(admin)
        .execute(s.pool())
        .await
        .unwrap();
    let admissions = s.admissions(admission(s.p()));
    for _ in 0..2 {
        s.intent(TO).await;
    }
    while s.drain(&admissions).await > 0 {}

    let company = s.fixture.target.company.as_uuid();
    let owner = PrincipalId::new(
        sqlx::query_scalar("SELECT id FROM principals WHERE company_id = $1 AND user_id = $2")
            .bind(company)
            .bind(s.owner().user_id())
            .fetch_one(s.pool())
            .await
            .unwrap(),
    );
    fn list(
        company_id: Uuid,
        principal_id: PrincipalId,
        view: AttentionView,
        visible_channel_ids: &[Uuid],
    ) -> AttentionQuery<'_> {
        AttentionQuery {
            company_id,
            principal_id,
            visible_channel_ids,
            manager: true,
            view,
            cursor: None,
            limit: 100,
        }
    }
    let channels = [s.channel];
    let team = s
        .p()
        .list_attention(list(company, owner, AttentionView::TeamWork, &channels))
        .await
        .unwrap()
        .items;
    let refusals: Vec<_> = team
        .iter()
        .filter(|item| item.source_kind == AttentionSourceKind::BindingRefusal)
        .collect();
    assert_eq!(refusals.len(), 1, "one row per binding");
    let row = refusals[0];
    assert_eq!(row.source_id, removed.as_uuid());
    assert_ne!(row.source_id, sibling.as_uuid());
    assert_eq!(row.channel_id, Some(s.channel));
    assert_eq!(row.version, 2);
    assert_eq!(
        row.next_action,
        "2 refusals in the last 7 days; latest: unavailable"
    );
    assert_eq!(
        row.href,
        Some(format!(
            "/ui/workflow-bindings/{}?company_id={company}",
            removed.as_uuid()
        ))
    );
    for view in [AttentionView::MyWork, AttentionView::Unassigned] {
        assert!(
            s.p()
                .list_attention(list(company, owner, view, &channels))
                .await
                .unwrap()
                .items
                .iter()
                .all(|item| item.source_kind != AttentionSourceKind::BindingRefusal),
            "{view:?}"
        );
    }
    assert!(
        s.p()
            .list_attention(list(company, owner, AttentionView::TeamWork, &[]))
            .await
            .unwrap()
            .items
            .is_empty(),
        "an unreadable channel's refusals are not listed"
    );
    assert_eq!(
        s.p()
            .operational_summary(company, &channels)
            .await
            .unwrap()
            .binding_refusal_count,
        2
    );
}

/// p08 D4.6 over a real admission: one message, two admitted bindings and a refused one. The
/// simulation reads every outcome in binding id order with the refusal's reason, the mailbox
/// lookup gives the message both runs in the same order, the thread's activity is its newest
/// unfinished run, and another company reads nothing.
#[tokio::test]
async fn message_outcomes_lookup_and_activity_read_every_binding_of_a_message() {
    use crate::application::{
        message_outcomes::MessageOutcomeReader, thread_activity::ThreadActivityReader,
    };
    use crate::domain::workflow::RunState;
    use crate::entities::{message_outcome::BindingOutcome, thread_activity::ThreadActivity};

    let s = Intents::new().await;
    let admin = member(&s.fixture, "admin").await;
    let refused = s.binding(MessageTrigger::Any, Some(admin)).await;
    let first = s.binding(MessageTrigger::Any, None).await;
    let second = s.binding(MessageTrigger::Any, None).await;
    sqlx::query("DELETE FROM company_members WHERE company_id = $1 AND user_id = $2")
        .bind(s.fixture.target.company.as_uuid())
        .bind(admin)
        .execute(s.pool())
        .await
        .unwrap();
    let intent = s.intent(TO).await;
    let message: Uuid =
        sqlx::query_scalar("SELECT message_id FROM workflow_message_intents WHERE id = $1")
            .bind(intent)
            .fetch_one(s.pool())
            .await
            .unwrap();
    s.drain(&s.admissions(admission(s.p()))).await;
    let company = s.fixture.target.company.as_uuid();
    let (first_run, second_run) = (s.runs(first).await[0], s.runs(second).await[0]);

    let outcomes = s
        .p()
        .message_outcomes(company, s.thread, &[message])
        .await
        .unwrap();
    let mut bindings = vec![refused.as_uuid(), first.as_uuid(), second.as_uuid()];
    bindings.sort();
    assert_eq!(
        outcomes
            .iter()
            .map(|outcome| outcome.binding_id)
            .collect::<Vec<_>>(),
        bindings,
        "every binding's outcome, in binding id order"
    );
    for outcome in &outcomes {
        assert_eq!(outcome.message_id, message);
        assert!(!outcome.workflow_title.is_empty());
        match &outcome.outcome {
            BindingOutcome::Refused { reason } => {
                assert_eq!(outcome.binding_id, refused.as_uuid());
                assert_eq!(reason, "unavailable");
            }
            BindingOutcome::Admitted(run) => {
                let expected = if outcome.binding_id == first.as_uuid() {
                    first_run
                } else {
                    second_run
                };
                assert_eq!(run.run_id, expected);
                assert_eq!(run.state, RunState::Queued);
                assert!(!run.executions.is_empty(), "the admitted first step");
                assert!(run.model_usage.is_empty());
            }
            BindingOutcome::Failed { .. } => panic!("no admission failed"),
        }
    }
    assert!(
        s.p()
            .message_outcomes(Uuid::new_v4(), s.thread, &[message])
            .await
            .unwrap()
            .is_empty(),
        "another company reads nothing"
    );

    let mut admitted = vec![(first.as_uuid(), first_run), (second.as_uuid(), second_run)];
    admitted.sort();
    let expected_runs: Vec<Uuid> = admitted.into_iter().map(|(_, run)| run).collect();
    let views = s.p().list_thread_message_views(s.thread).await.unwrap();
    let view = views
        .iter()
        .find(|view| view.canonical_id.as_uuid() == message)
        .expect("the message is in its thread");
    assert_eq!(
        view.run_ids, expected_runs,
        "the admitted runs in binding order"
    );
    let single = s
        .p()
        .get_thread_message_view(s.thread, CanonicalMessageId::new(message))
        .await
        .unwrap()
        .unwrap();
    assert_eq!(single.run_ids, expected_runs);

    assert_independent_thread_runs(&s, company, message, &expected_runs).await;
    let activity = s.p().thread_activity(company, &[s.thread]).await.unwrap();
    assert_eq!(activity[&s.thread].activity, Some(ThreadActivity::Queued));
    assert!(expected_runs.contains(&activity[&s.thread].run_id));
    assert!(
        s.p()
            .thread_activity(Uuid::new_v4(), &[s.thread])
            .await
            .unwrap()
            .is_empty()
    );
}

/// The next notification on each of the two channels, in channel order.
async fn both(listener: &mut sqlx::postgres::PgListener) -> Vec<(String, serde_json::Value)> {
    let mut heard = vec![
        super::super::next_notification(listener).await,
        super::super::next_notification(listener).await,
    ];
    heard.sort_by(|left, right| left.0.cmp(&right.0));
    heard
}

/// p08 D4.1 over a real LISTEN connection, for a run on a thread: its insert and its state and
/// waiting-reason changes notify both `workflow_run_changed` and `thread_activity`; an update of
/// any other column is silent.
#[tokio::test]
async fn workflow_run_triggers_notify_a_threads_run_on_both_channels() {
    let s = Intents::new().await;
    let mut listener = super::super::run_listener(s.pool()).await;
    let binding = s.binding(MessageTrigger::Any, None).await;
    s.intent(TO).await;
    s.drain(&s.admissions(admission(s.p()))).await;
    let run = s.runs(binding).await[0];
    let company = s.fixture.target.company.as_uuid();
    // Channel order: `thread_activity` sorts first.
    let expected = |state: &str, waiting_reason: Option<&str>| {
        vec![
            (
                "thread_activity".to_string(),
                json!({"company_id":company,"channel_id":s.channel,"thread_id":s.thread}),
            ),
            (
                "workflow_run_changed".to_string(),
                json!({"company_id":company,"channel_id":s.channel,"thread_id":s.thread,
                    "run_id":run,"state":state,"waiting_reason":waiting_reason}),
            ),
        ]
    };
    assert_eq!(
        both(&mut listener).await,
        expected("queued", None),
        "the insert"
    );

    for (update, state, waiting_reason) in [
        (
            "UPDATE workflow_runs SET state = 'running' WHERE company_id = $1 AND id = $2",
            "running",
            None,
        ),
        (
            "UPDATE workflow_runs SET state = 'waiting', waiting_reason = 'event' WHERE company_id = $1 AND id = $2",
            "waiting",
            Some("event"),
        ),
        (
            "UPDATE workflow_runs SET waiting_reason = 'timer' WHERE company_id = $1 AND id = $2",
            "waiting",
            Some("timer"),
        ),
    ] {
        sqlx::query(update)
            .bind(company)
            .bind(run)
            .execute(s.pool())
            .await
            .unwrap();
        assert_eq!(
            both(&mut listener).await,
            expected(state, waiting_reason),
            "{update}"
        );
    }

    sqlx::query("UPDATE workflow_runs SET deadline = deadline - interval '1 second' WHERE company_id = $1 AND id = $2")
        .bind(company)
        .bind(run)
        .execute(s.pool())
        .await
        .unwrap();
    super::super::assert_silent_until_marker(s.pool(), &mut listener).await;
}

async fn assert_independent_thread_runs(
    s: &Intents,
    company: Uuid,
    message: Uuid,
    expected_runs: &[Uuid],
) {
    use crate::application::thread_activity::ThreadActivityReader;
    use crate::entities::thread_activity::ThreadActivity;
    let runs = s
        .p()
        .thread_runs(company, s.thread, s.owner().user_id())
        .await
        .unwrap();
    assert_eq!(runs.items.len(), 2);
    assert!(!runs.has_more);
    assert_eq!(
        runs.message_runs
            .iter()
            .find(|row| row.canonical_id == message)
            .unwrap()
            .run_ids,
        expected_runs
    );
    for run in &runs.items {
        assert!(expected_runs.contains(&run.run_id));
        assert_eq!(run.activity, ThreadActivity::Queued);
        assert!(!run.title.is_empty());
    }
    assert!(
        s.p()
            .thread_runs(Uuid::new_v4(), s.thread, s.owner().user_id())
            .await
            .unwrap()
            .items
            .is_empty()
    );
}

#[tokio::test]
async fn thread_runs_are_bounded_and_report_truncation() {
    use crate::application::thread_activity::{THREAD_RUN_LIMIT, ThreadActivityReader};
    let s = Intents::new().await;
    let binding = s.binding(MessageTrigger::Any, None).await;
    for _ in 0..=THREAD_RUN_LIMIT {
        s.intent(TO).await;
        assert_eq!(s.drain(&s.admissions(admission(s.p()))).await, 1);
    }
    assert_eq!(s.runs(binding).await.len(), THREAD_RUN_LIMIT + 1);
    let listed = s
        .p()
        .thread_runs(
            s.fixture.target.company.as_uuid(),
            s.thread,
            s.owner().user_id(),
        )
        .await
        .unwrap();
    assert_eq!(listed.items.len(), THREAD_RUN_LIMIT);
    assert!(listed.has_more);
}

#[tokio::test]
async fn thread_attribution_links_require_access_to_the_producing_run() {
    use crate::application::thread_activity::ThreadActivityReader;
    let s = Intents::new().await;
    let admin = member(&s.fixture, "admin").await;
    let reader = member(&s.fixture, "member").await;
    let binding = s.binding(MessageTrigger::Any, None).await;
    s.intent(TO).await;
    s.drain(&s.admissions(admission(s.p()))).await;
    let run = s.runs(binding).await[0];
    let company = s.fixture.target.company.as_uuid();
    assert!(
        s.p()
            .thread_runs(company, s.thread, admin)
            .await
            .unwrap()
            .inspectable
            .contains(&run)
    );
    assert!(
        s.p()
            .thread_runs(company, s.thread, reader)
            .await
            .unwrap()
            .inspectable
            .is_empty()
    );
    sqlx::query("UPDATE channels SET access_mode='allowlist' WHERE id=$1")
        .bind(s.channel)
        .execute(s.pool())
        .await
        .unwrap();
    assert!(
        s.p()
            .thread_runs(company, s.thread, admin)
            .await
            .unwrap()
            .inspectable
            .is_empty()
    );
    assert!(
        s.p()
            .thread_runs(company, s.thread, s.owner().user_id())
            .await
            .unwrap()
            .inspectable
            .contains(&run)
    );
}
