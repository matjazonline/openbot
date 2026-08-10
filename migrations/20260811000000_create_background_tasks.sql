-- up
CREATE TABLE IF NOT EXISTS background_tasks (
    id UUID PRIMARY KEY,
    company_id UUID NOT NULL REFERENCES companies(id) ON DELETE CASCADE,
    workflow_id UUID NOT NULL REFERENCES workflows(id) ON DELETE CASCADE,
    thread_id UUID REFERENCES threads(id) ON DELETE SET NULL,
    task_type VARCHAR(50) NOT NULL,
    status VARCHAR(20) NOT NULL,
    payload JSONB NOT NULL,
    retry_count INT NOT NULL DEFAULT 0,
    max_retries INT NOT NULL DEFAULT 3,
    last_error TEXT,
    run_at TIMESTAMP NOT NULL DEFAULT CURRENT_TIMESTAMP,
    created_at TIMESTAMP NOT NULL DEFAULT CURRENT_TIMESTAMP,
    updated_at TIMESTAMP NOT NULL DEFAULT CURRENT_TIMESTAMP
);

CREATE INDEX IF NOT EXISTS idx_bg_tasks_company_id ON background_tasks(company_id);
CREATE INDEX IF NOT EXISTS idx_bg_tasks_workflow_id ON background_tasks(workflow_id);
CREATE INDEX IF NOT EXISTS idx_bg_tasks_status_run_at ON background_tasks(status, run_at);
