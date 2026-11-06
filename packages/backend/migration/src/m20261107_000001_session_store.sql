-- Sessions move to tower-sessions (through axum-login): one row per session, keyed by the SHA-256 (hex) of
-- the session id in the cookie, never the id itself, so a leaked database or backup holds no live session.
-- `data` is the session's JSON (who is signed in, and a hash of their password and two-factor secret);
-- `expires_at` is when it ends. The old table is dropped: everyone signs in again once.
DROP TABLE IF EXISTS "sessions";

CREATE TABLE "sessions" (
    "id_hash" TEXT NOT NULL,
    "data" TEXT NOT NULL,
    "expires_at" datetime_text NOT NULL,
    PRIMARY KEY ("id_hash")
);

CREATE INDEX "index_sessions_by_expires_at" ON "sessions" ("expires_at");
