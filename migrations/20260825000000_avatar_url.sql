-- Profile pictures for the two things the `/ui` pages put a face on: the people using the app and
-- the agents answering in it. Both are optional -- a row without one is rendered as the letter
-- bubble that has always been there, so nothing has to be backfilled.

ALTER TABLE users ADD COLUMN avatar_url TEXT;
ALTER TABLE agents ADD COLUMN avatar_url TEXT;

-- The column is written from a form field and read straight into an `<img src>`, so the one scheme
-- rule the renderer relies on is enforced where it cannot be bypassed.
ALTER TABLE users ADD CONSTRAINT users_avatar_url_scheme_check
    CHECK (avatar_url IS NULL OR avatar_url ~ '^https?://');
ALTER TABLE agents ADD CONSTRAINT agents_avatar_url_scheme_check
    CHECK (avatar_url IS NULL OR avatar_url ~ '^https?://');
