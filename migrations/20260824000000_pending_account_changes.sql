-- A change to an account that is waiting on a code mailed out to prove it was really asked for.
--
-- The two kinds prove different things and so mail the code to different places: an email change
-- sends it to the *new* address (proving the account owner can read it), a password change sends
-- it to the address the account already has. That is why the new address lives here rather than
-- being written to `users` and confirmed in place -- an unconfirmed address must never be one the
-- account can sign in or receive mail as.
CREATE TABLE pending_account_changes (
    user_id UUID NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    kind TEXT NOT NULL,
    -- Set for a 'email' change and null for a 'password' one, and vice versa: the CHECK below is
    -- what keeps a row from claiming to be one kind while carrying the other's payload.
    new_email CITEXT,
    new_password_hash TEXT,
    confirmation_code_hash TEXT NOT NULL,
    expires_at TIMESTAMPTZ NOT NULL,
    created_at TIMESTAMPTZ NOT NULL DEFAULT CURRENT_TIMESTAMP,
    -- One pending change of each kind per account. Asking again replaces the earlier request, so
    -- an abandoned code cannot still be confirmed after a second one was sent.
    PRIMARY KEY (user_id, kind),
    CONSTRAINT pending_account_changes_kind_check CHECK (kind IN ('email', 'password')),
    CONSTRAINT pending_account_changes_payload_matches_kind CHECK (
        (kind = 'email' AND new_email IS NOT NULL AND new_password_hash IS NULL)
        OR (kind = 'password' AND new_password_hash IS NOT NULL AND new_email IS NULL)
    ),
    CONSTRAINT pending_account_changes_email_not_blank
        CHECK (new_email IS NULL OR btrim(new_email::text) <> '')
);
