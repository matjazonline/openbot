//! Read-only deferred probes abort only after the actual owner reached commit.
use super::*;

#[derive(Clone, Copy)]
pub(super) enum ProbeKind {
    Candidate,
    Refusal,
}

impl ProbeKind {
    pub(super) fn diagnostic(self) -> &'static str {
        match self {
            Self::Candidate => "injected deferred candidate claim commit",
            Self::Refusal => "injected deferred budget refusal commit",
        }
    }
    fn table(self) -> &'static str {
        match self {
            Self::Candidate => "task_attempts",
            Self::Refusal => "workflow_action_claim_budget_refusals",
        }
    }
}

pub(super) struct Probe {
    name: String,
    kind: ProbeKind,
}

fn literal(value: &str) -> String {
    format!("'{}'", value.replace('\'', "''"))
}

const COMMON: &str = r#"
    IF NOT EXISTS(SELECT 1 FROM workflow_action_claim_episodes AS episode
        WHERE episode.company_id=TG_ARGV[0]::uuid AND episode.run_id=TG_ARGV[1]::uuid
            AND episode.execution_id=TG_ARGV[2]::uuid AND episode.job_id=TG_ARGV[3]::uuid
            AND episode.command_key=TG_ARGV[4] AND episode.retired_attempt=TG_ARGV[5]::integer
            AND to_jsonb(episode)=TG_ARGV[6]::jsonb)
    THEN RAISE EXCEPTION 'probe prerequisite: exact unchanged episode'; END IF;
    IF NOT EXISTS(SELECT 1 FROM task_attempts AS retired
        WHERE retired.task_id=TG_ARGV[3]::uuid AND retired.attempt_number=TG_ARGV[5]::integer
            AND to_jsonb(retired)=TG_ARGV[7]::jsonb)
    THEN RAISE EXCEPTION 'probe prerequisite: unchanged retired attempt'; END IF;
    IF NOT EXISTS(SELECT 1 FROM workflow_executions AS execution
        WHERE execution.company_id=TG_ARGV[0]::uuid AND execution.run_id=TG_ARGV[1]::uuid
            AND execution.id=TG_ARGV[2]::uuid AND execution.activated_at IS NOT NULL
            AND execution.completed_at IS NULL AND to_jsonb(execution)=TG_ARGV[8]::jsonb)
    THEN RAISE EXCEPTION 'probe prerequisite: frozen activated uncompleted execution'; END IF;
"#;

const CANDIDATE: &str = r#"
    IF NEW.task_id<>TG_ARGV[3]::uuid THEN RETURN NULL; END IF;
    IF NEW.attempt_number<>TG_ARGV[5]::integer+1 OR NEW.status<>'processing'
        OR NEW.worker_id IS DISTINCT FROM TG_ARGV[9]::uuid OR NEW.execution_generation IS NULL
        OR NEW.finished_at IS NOT NULL
    THEN RAISE EXCEPTION 'probe prerequisite: exact next processing attempt'; END IF;
    IF NOT EXISTS(SELECT 1 FROM background_tasks AS job
        JOIN workflow_runs AS owner ON owner.company_id=job.company_id AND owner.id=TG_ARGV[1]::uuid
        WHERE job.company_id=TG_ARGV[0]::uuid AND job.id=TG_ARGV[3]::uuid
            AND job.workflow_execution_id=TG_ARGV[2]::uuid AND job.queue_kind='workflow'
            AND job.status='processing' AND job.retry_count=TG_ARGV[5]::integer
            AND job.worker_id=NEW.worker_id AND job.execution_generation=NEW.execution_generation
            AND job.lock_expires_at>clock_timestamp() AND owner.deadline>clock_timestamp()
            AND owner.state='running')
    THEN RAISE EXCEPTION 'probe prerequisite: installed live matching lease and run'; END IF;
    IF (SELECT count(*) FROM task_attempts AS later
        WHERE later.task_id=TG_ARGV[3]::uuid AND later.attempt_number>TG_ARGV[5]::integer)<>1
        OR EXISTS(SELECT 1 FROM workflow_action_claim_budget_refusals AS refusal
            WHERE refusal.company_id=TG_ARGV[0]::uuid AND refusal.command_key=TG_ARGV[4])
        OR NOT workflow_action_reconciliation_budget_eligible(TG_ARGV[0]::uuid,TG_ARGV[1]::uuid)
    THEN RAISE EXCEPTION 'probe prerequisite: one candidate with headroom and zero refusal'; END IF;
"#;

const REFUSAL: &str = r#"
    IF NEW.job_id<>TG_ARGV[3]::uuid THEN RETURN NULL; END IF;
    IF NEW.company_id<>TG_ARGV[0]::uuid OR NEW.run_id<>TG_ARGV[1]::uuid
        OR NEW.execution_id<>TG_ARGV[2]::uuid OR NEW.command_key<>TG_ARGV[4]
        OR NEW.retired_attempt<>TG_ARGV[5]::integer
    THEN RAISE EXCEPTION 'probe prerequisite: exact refusal identity'; END IF;
    IF workflow_action_reconciliation_budget_eligible(NEW.company_id,NEW.run_id)
    THEN RAISE EXCEPTION 'probe prerequisite: actual budget ineligibility'; END IF;
    IF EXISTS(SELECT 1 FROM task_attempts AS later WHERE later.task_id=NEW.job_id
        AND later.attempt_number>NEW.retired_attempt)
    THEN RAISE EXCEPTION 'probe prerequisite: no greater attempt'; END IF;
    IF NOT EXISTS(SELECT 1 FROM background_tasks AS job WHERE job.company_id=NEW.company_id
        AND job.id=NEW.job_id AND job.workflow_execution_id=NEW.execution_id AND job.queue_kind='workflow'
        AND job.status='failed' AND job.retry_count=NEW.retired_attempt AND job.worker_id IS NULL
        AND job.execution_generation IS NULL AND job.lock_expires_at IS NULL)
    THEN RAISE EXCEPTION 'probe prerequisite: genuine lease-free failed job'; END IF;
    IF NOT EXISTS(SELECT 1 FROM workflow_action_claim_retirement_witnesses AS witness
        JOIN workflow_runs AS owner ON owner.company_id=witness.company_id AND owner.id=witness.run_id
        JOIN workflow_run_events AS audit ON audit.company_id=witness.company_id AND audit.run_id=witness.run_id
            AND audit.sequence=witness.audit_sequence
        WHERE witness.company_id=NEW.company_id AND witness.run_id=NEW.run_id
            AND witness.execution_id=NEW.execution_id AND witness.job_id=NEW.job_id
            AND witness.command_key=NEW.command_key AND witness.retired_attempt=NEW.retired_attempt
            AND witness.transaction_id=pg_current_xact_id() AND witness.retirement_confirmed
            AND witness.audit_sequence=NEW.audit_sequence AND witness.deadline>clock_timestamp()
            AND owner.deadline>clock_timestamp()
            AND ((owner.state='failed' AND owner.terminal_execution_id=NEW.execution_id
                AND owner.waiting_reason IS NULL AND workflow_action_retry_safe(NEW.company_id,NEW.execution_id) IS DISTINCT FROM false)
                OR (owner.state='waiting' AND owner.waiting_reason='reconciliation'
                    AND owner.terminal_execution_id IS NULL AND workflow_action_retry_safe(NEW.company_id,NEW.execution_id) IS FALSE))
            AND audit.execution_id=NEW.execution_id AND audit.event_kind='workflow.root_budget_exhausted')
    THEN RAISE EXCEPTION 'probe prerequisite: current confirmed retirement and exact protected audit'; END IF;
"#;

fn arguments(s: &Scheduled, before: &Value, claimant: WorkflowWorkerId) -> String {
    let scope = s.request.fence.scope;
    let episode = &before["workflow_action_claim_episodes"][0];
    let retired = before["task_attempts"]
        .as_array()
        .unwrap()
        .iter()
        .find(|row| {
            row["task_id"] == json!(scope.job.0)
                && row["attempt_number"] == json!(s.request.fence.attempt.0)
        })
        .unwrap();
    let execution = before["workflow_executions"]
        .as_array()
        .unwrap()
        .iter()
        .find(|row| row["id"] == json!(scope.execution.as_uuid()))
        .unwrap();
    [
        scope.company.as_uuid().to_string(),
        scope.run.as_uuid().to_string(),
        scope.execution.as_uuid().to_string(),
        scope.job.0.to_string(),
        s.command.command_key.as_str().to_owned(),
        s.request.fence.attempt.0.to_string(),
        episode.to_string(),
        retired.to_string(),
        execution.to_string(),
        claimant.0.to_string(),
    ]
    .iter()
    .map(|value| literal(value))
    .collect::<Vec<_>>()
    .join(",")
}

async fn absent(s: &Scheduled, name: &str) {
    let absent: bool = sqlx::query_scalar("SELECT NOT EXISTS(SELECT 1 FROM pg_trigger AS trigger WHERE trigger.tgname=$1) AND NOT EXISTS(SELECT 1 FROM pg_proc AS function WHERE function.proname=$1)")
        .bind(name).fetch_one(s.fixture.persistence().pool()).await.unwrap();
    assert!(absent, "probe trigger and function must both be absent");
}

pub(super) async fn install_probe(
    s: &Scheduled,
    before: &Value,
    claimant: WorkflowWorkerId,
    kind: ProbeKind,
) -> Probe {
    let name = format!("zz_claim_abort_{}", Uuid::new_v4().simple());
    absent(s, &name).await;
    let branch = match kind {
        ProbeKind::Candidate => CANDIDATE,
        ProbeKind::Refusal => REFUSAL,
    };
    let args = arguments(s, before, claimant);
    let sql = format!(
        "CREATE FUNCTION {name}() RETURNS trigger LANGUAGE plpgsql AS $probe$ BEGIN {branch} {COMMON} RAISE EXCEPTION '{}'; END $probe$; CREATE CONSTRAINT TRIGGER {name} AFTER INSERT ON {} DEFERRABLE INITIALLY DEFERRED FOR EACH ROW EXECUTE FUNCTION {name}({args});",
        kind.diagnostic(),
        kind.table()
    );
    sqlx::raw_sql(&sql)
        .execute(s.fixture.persistence().pool())
        .await
        .unwrap();
    let definition: bool = sqlx::query_scalar("SELECT count(*)=1 AND bool_and(trigger.tgdeferrable AND trigger.tginitdeferred AND trigger.tgenabled='O' AND trigger.tgtype=5 AND trigger.tgconstraint<>0 AND trigger.tgrelid=to_regclass($2)) FROM pg_trigger AS trigger WHERE trigger.tgname=$1")
        .bind(&name).bind(kind.table()).fetch_one(s.fixture.persistence().pool()).await.unwrap();
    assert!(
        definition,
        "exact AFTER INSERT constraint probe is enabled and initially deferred"
    );
    Probe { name, kind }
}

pub(super) async fn remove_probe(s: &Scheduled, probe: Probe) {
    sqlx::raw_sql(&format!(
        "DROP TRIGGER {} ON {}; DROP FUNCTION {}();",
        probe.name,
        probe.kind.table(),
        probe.name
    ))
    .execute(s.fixture.persistence().pool())
    .await
    .unwrap();
    absent(s, &probe.name).await;
}
