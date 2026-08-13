-- Initial consolidated schema

-- 1. Users
CREATE TABLE IF NOT EXISTS users (
    id UUID PRIMARY KEY,
    username VARCHAR(50) UNIQUE NOT NULL,
    email VARCHAR(100) UNIQUE NOT NULL,
    password_hash VARCHAR(255) NOT NULL,
    created_at TIMESTAMP DEFAULT CURRENT_TIMESTAMP
);

-- 2. Companies
CREATE TABLE IF NOT EXISTS companies (
    id UUID PRIMARY KEY,
    user_id UUID NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    name VARCHAR(100) NOT NULL,
    slug VARCHAR(100) NOT NULL,
    api_key VARCHAR(255),
    provider VARCHAR(100),
    model VARCHAR(100),
    enable_llm_spam_guardrail BOOLEAN DEFAULT NULL,
    created_at TIMESTAMP DEFAULT CURRENT_TIMESTAMP
);

CREATE INDEX IF NOT EXISTS idx_companies_slug ON companies(slug);
CREATE INDEX IF NOT EXISTS idx_companies_user_id ON companies(user_id);

-- 3. Company Invites & Members
CREATE TABLE IF NOT EXISTS company_invites (
    id UUID PRIMARY KEY,
    company_id UUID NOT NULL REFERENCES companies(id) ON DELETE CASCADE,
    email VARCHAR(100) NOT NULL,
    status VARCHAR(20) NOT NULL DEFAULT 'pending',
    created_at TIMESTAMP DEFAULT CURRENT_TIMESTAMP,
    CONSTRAINT uk_company_invites_company_email UNIQUE (company_id, email)
);

CREATE INDEX IF NOT EXISTS idx_company_invites_company_id ON company_invites(company_id);
CREATE INDEX IF NOT EXISTS idx_company_invites_email ON company_invites(email);

CREATE TABLE IF NOT EXISTS company_members (
    id UUID PRIMARY KEY,
    company_id UUID NOT NULL REFERENCES companies(id) ON DELETE CASCADE,
    user_id UUID NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    role VARCHAR(20) NOT NULL DEFAULT 'member',
    created_at TIMESTAMP DEFAULT CURRENT_TIMESTAMP,
    CONSTRAINT uk_company_members_company_user UNIQUE (company_id, user_id)
);

CREATE INDEX IF NOT EXISTS idx_company_members_company_id ON company_members(company_id);
CREATE INDEX IF NOT EXISTS idx_company_members_user_id ON company_members(user_id);

-- 4. Agents
CREATE TABLE IF NOT EXISTS agents (
    id UUID PRIMARY KEY,
    company_id UUID NOT NULL REFERENCES companies(id) ON DELETE CASCADE,
    name VARCHAR(100) NOT NULL,
    slug VARCHAR(100) NOT NULL,
    provider VARCHAR(100),
    model VARCHAR(100),
    api_key TEXT,
    system_prompt TEXT,
    config_json JSONB,
    created_at TIMESTAMP DEFAULT CURRENT_TIMESTAMP
);

CREATE INDEX IF NOT EXISTS idx_agents_company_id ON agents(company_id);
CREATE INDEX IF NOT EXISTS idx_agents_slug ON agents(slug);
CREATE UNIQUE INDEX IF NOT EXISTS idx_agents_company_id_slug ON agents(company_id, slug);

-- 5. Channels
CREATE TABLE IF NOT EXISTS channels (
    id UUID PRIMARY KEY,
    company_id UUID NOT NULL REFERENCES companies(id) ON DELETE CASCADE,
    name VARCHAR(100) NOT NULL,
    slug VARCHAR(100) NOT NULL,
    participant_emails TEXT[],
    channel_config JSONB,
    api_key VARCHAR(255),
    provider VARCHAR(100),
    model VARCHAR(100),
    agent_ids UUID[],
    created_at TIMESTAMP DEFAULT CURRENT_TIMESTAMP
);

CREATE INDEX IF NOT EXISTS idx_channels_slug ON channels(slug);
CREATE INDEX IF NOT EXISTS idx_channels_company_id ON channels(company_id);

-- 6. Threads & Messages
CREATE TABLE IF NOT EXISTS threads (
    id UUID PRIMARY KEY,
    channel_id UUID NOT NULL REFERENCES channels(id) ON DELETE CASCADE,
    subject VARCHAR(255) NOT NULL,
    participant_emails TEXT[] NOT NULL DEFAULT '{}',
    created_at TIMESTAMP NOT NULL DEFAULT CURRENT_TIMESTAMP,
    updated_at TIMESTAMP NOT NULL DEFAULT CURRENT_TIMESTAMP
);

CREATE TABLE IF NOT EXISTS messages (
    id UUID PRIMARY KEY,
    thread_id UUID NOT NULL REFERENCES threads(id) ON DELETE CASCADE,
    message_id VARCHAR(255) NOT NULL UNIQUE,
    in_reply_to VARCHAR(255),
    references_list TEXT[] NOT NULL DEFAULT '{}',
    sender VARCHAR(255) NOT NULL,
    recipients_to TEXT[] NOT NULL DEFAULT '{}',
    recipients_cc TEXT[] NOT NULL DEFAULT '{}',
    subject VARCHAR(255) NOT NULL,
    clean_text_body TEXT NOT NULL,
    raw_text_body TEXT,
    raw_html_body TEXT,
    attachments JSONB,
    direction VARCHAR(20) NOT NULL,
    role VARCHAR(20) NOT NULL,
    thread_index VARCHAR(255),
    created_at TIMESTAMP NOT NULL DEFAULT CURRENT_TIMESTAMP
);

CREATE INDEX IF NOT EXISTS idx_threads_channel_id ON threads(channel_id);
CREATE INDEX IF NOT EXISTS idx_messages_thread_id ON messages(thread_id);
CREATE INDEX IF NOT EXISTS idx_messages_message_id ON messages(message_id);
CREATE INDEX IF NOT EXISTS idx_messages_in_reply_to ON messages(in_reply_to);

-- 7. Background Tasks
CREATE TABLE IF NOT EXISTS background_tasks (
    id UUID PRIMARY KEY,
    company_id UUID NOT NULL REFERENCES companies(id) ON DELETE CASCADE,
    channel_id UUID NOT NULL REFERENCES channels(id) ON DELETE CASCADE,
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
CREATE INDEX IF NOT EXISTS idx_bg_tasks_channel_id ON background_tasks(channel_id);
CREATE INDEX IF NOT EXISTS idx_bg_tasks_status_run_at ON background_tasks(status, run_at);

-- 8. Human Approvals
CREATE TABLE IF NOT EXISTS human_approvals (
    id UUID PRIMARY KEY,
    company_id UUID NOT NULL REFERENCES companies(id) ON DELETE CASCADE,
    channel_id UUID NOT NULL REFERENCES channels(id) ON DELETE CASCADE,
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
CREATE INDEX IF NOT EXISTS idx_human_approvals_channel ON human_approvals(channel_id);
CREATE INDEX IF NOT EXISTS idx_human_approvals_status ON human_approvals(status);
