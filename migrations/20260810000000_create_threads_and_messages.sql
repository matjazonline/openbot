-- up
CREATE TABLE IF NOT EXISTS threads (
    id UUID PRIMARY KEY,
    workflow_id UUID NOT NULL REFERENCES workflows(id) ON DELETE CASCADE,
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

CREATE INDEX IF NOT EXISTS idx_threads_workflow_id ON threads(workflow_id);
CREATE INDEX IF NOT EXISTS idx_messages_thread_id ON messages(thread_id);
CREATE INDEX IF NOT EXISTS idx_messages_message_id ON messages(message_id);
CREATE INDEX IF NOT EXISTS idx_messages_in_reply_to ON messages(in_reply_to);
