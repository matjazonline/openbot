-- up
CREATE TABLE IF NOT EXISTS human_approvals (
    id UUID PRIMARY KEY,
    company_id UUID NOT NULL REFERENCES companies(id) ON DELETE CASCADE,
    workflow_id UUID NOT NULL REFERENCES workflows(id) ON DELETE CASCADE,
    thread_id UUID REFERENCES threads(id) ON DELETE SET NULL,
    task_id UUID REFERENCES background_tasks(id) ON DELETE SET NULL,
    step_key VARCHAR(64) NOT NULL,
    approver_email VARCHAR(255) NOT NULL,
    action_type VARCHAR(100) NOT NULL,
    action_title VARCHAR(255) NOT NULL,
    action_summary TEXT NOT NULL,
    payload JSONB NOT NULL,
    token VARCHAR(64) NOT NULL UNIQUE,
    status VARCHAR(20) NOT NULL DEFAULT 'pending',
    expires_at TIMESTAMP NOT NULL,
    created_at TIMESTAMP NOT NULL DEFAULT CURRENT_TIMESTAMP,
    updated_at TIMESTAMP NOT NULL DEFAULT CURRENT_TIMESTAMP
);

CREATE INDEX IF NOT EXISTS idx_human_approvals_thread_step ON human_approvals(thread_id, step_key);
CREATE INDEX IF NOT EXISTS idx_human_approvals_token ON human_approvals(token);
CREATE INDEX IF NOT EXISTS idx_human_approvals_workflow ON human_approvals(workflow_id);
CREATE INDEX IF NOT EXISTS idx_human_approvals_status ON human_approvals(status);
