CREATE TABLE pending_user_registrations (
    email CITEXT PRIMARY KEY,
    username CITEXT NOT NULL,
    password_hash TEXT NOT NULL,
    confirmation_code_hash TEXT NOT NULL,
    expires_at TIMESTAMPTZ NOT NULL,
    created_at TIMESTAMPTZ NOT NULL DEFAULT CURRENT_TIMESTAMP,
    CONSTRAINT pending_user_registrations_username_not_blank CHECK (btrim(username::text) <> ''),
    CONSTRAINT pending_user_registrations_email_not_blank CHECK (btrim(email::text) <> '')
);

CREATE UNIQUE INDEX pending_user_registrations_username_key
    ON pending_user_registrations (username);
