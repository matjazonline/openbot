ALTER TABLE user_login_methods
    DROP CONSTRAINT user_login_methods_provider_check,
    DROP CONSTRAINT user_login_methods_subject_check;

ALTER TABLE user_login_methods
    ADD CONSTRAINT user_login_methods_provider_check
        CHECK (provider IN ('password', 'google', 'apple')),
    ADD CONSTRAINT user_login_methods_subject_check CHECK (
        (provider = 'password' AND provider_subject IS NULL)
        OR (provider IN ('google', 'apple')
            AND provider_subject IS NOT NULL
            AND btrim(provider_subject) <> '')
    );
