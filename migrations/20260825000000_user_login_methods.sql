-- Authentication methods are explicit: finding the same email through another provider must not
-- silently turn that provider into a way into the account.
CREATE TABLE user_login_methods (
    user_id UUID NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    provider TEXT NOT NULL,
    provider_subject TEXT,
    created_at TIMESTAMPTZ NOT NULL DEFAULT CURRENT_TIMESTAMP,
    PRIMARY KEY (user_id, provider),
    CONSTRAINT user_login_methods_provider_check CHECK (provider IN ('password', 'google')),
    CONSTRAINT user_login_methods_subject_check CHECK (
        (provider = 'password' AND provider_subject IS NULL)
        OR (provider = 'google' AND provider_subject IS NOT NULL AND btrim(provider_subject) <> '')
    )
);

CREATE UNIQUE INDEX user_login_methods_provider_subject_key
    ON user_login_methods (provider, provider_subject)
    WHERE provider_subject IS NOT NULL;

-- Every account which predates explicit methods was registered with a password.
INSERT INTO user_login_methods (user_id, provider)
SELECT id, 'password' FROM users;
