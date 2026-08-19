-- Whether a trusted sender may pull CC'd outsiders onto this channel's threads. Off means the
-- channel is internal: outsiders never join a thread and never appear on an agent reply's Cc.
--
-- DEFAULT TRUE preserves the behaviour every existing channel already has.
ALTER TABLE channels
    ADD COLUMN add_3rd_party BOOLEAN NOT NULL DEFAULT TRUE;
