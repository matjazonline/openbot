-- A company gets a picture of its own, on the same terms as a user's or an agent's: an http(s)
-- URL or nothing, so what a page renders into an `<img src>` can never be an active scheme.
ALTER TABLE companies ADD COLUMN avatar_url TEXT;

ALTER TABLE companies
    ADD CONSTRAINT companies_avatar_url_scheme_check
    CHECK (avatar_url IS NULL OR avatar_url ~ '^https?://');
