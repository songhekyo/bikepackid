-- google_id (Google's stable `sub`) is our real identity key. A UNIQUE
-- constraint on email adds a failure mode with no real benefit: a reused
-- or recycled email address (deleted Workspace account, email changed and
-- reassigned) on a *different* Google account would make the login upsert
-- (ON CONFLICT (google_id)) violate this constraint and 500 instead of
-- just updating that user's row.
ALTER TABLE users DROP CONSTRAINT users_email_key;

CREATE INDEX users_email_idx ON users (email);
