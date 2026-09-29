-- Scheduling hints only. Existing jobs/attempts exclusively own execution.
-- No ancestor FKs: run-owning claims must never acquire company/other-run locks.
CREATE TABLE workflow_dispatch_demand (
    company_id uuid PRIMARY KEY,
    ticket bigint GENERATED ALWAYS AS IDENTITY UNIQUE,
    worker_id uuid NOT NULL UNIQUE,
    run_id uuid NOT NULL,
    execution_id uuid NOT NULL,
    job_id uuid NOT NULL,
    opportunity_until timestamptz,
    registered_at timestamptz NOT NULL DEFAULT clock_timestamp()
);
CREATE TABLE workflow_poll_workers (
    worker_id uuid PRIMARY KEY,
    after_company uuid,
    touched_at timestamptz NOT NULL DEFAULT clock_timestamp()
);
CREATE TABLE workflow_poll_companies (
    worker_id uuid NOT NULL REFERENCES workflow_poll_workers(worker_id) ON DELETE CASCADE,
    company_id uuid NOT NULL,
    after_job uuid,
    horizon timestamptz NOT NULL DEFAULT clock_timestamp(),
    PRIMARY KEY (worker_id, company_id)
);

-- Once capacity is offered, a newly eligible older company cannot preempt it.
CREATE UNIQUE INDEX workflow_dispatch_one_opportunity
    ON workflow_dispatch_demand ((true)) WHERE opportunity_until IS NOT NULL;
