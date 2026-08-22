-- What this channel is for, in one line. Read back to a teammate who mails an address that does
-- not exist, so they can find the channel they meant without asking anyone.
ALTER TABLE channels ADD COLUMN description TEXT;
