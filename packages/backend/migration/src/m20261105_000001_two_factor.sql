-- Two-factor sign-in with a time-based one-time password (TOTP, RFC 6238) and single-use recovery codes.
-- New tables only, so this runs as it is, and `IF NOT EXISTS` makes a repeat harmless.

-- A user's authenticator secret: at most one per user. `enabled_at` is NULL while it is being set up (the
-- secret was shown, no code was confirmed yet); a new setup replaces such a row. `last_step` is the newest
-- 30-second time step a code was accepted for, so a code cannot be used twice.
CREATE TABLE IF NOT EXISTS "user_totp" (
    "user_id" uuid_text NOT NULL,
    "secret" TEXT NOT NULL,
    "enabled_at" datetime_text,
    "last_step" INTEGER,
    "created_at" datetime_text NOT NULL,
    PRIMARY KEY ("user_id"),
    FOREIGN KEY ("user_id") REFERENCES "users" ("id") ON DELETE CASCADE
);

-- Recovery codes, each good for one sign-in. Only a SHA-256 hash is kept: the codes are random (80 bits), so
-- a slow hash would add nothing.
CREATE TABLE IF NOT EXISTS "user_recovery_codes" (
    "id" uuid_text NOT NULL,
    "user_id" uuid_text NOT NULL,
    "code_hash" TEXT NOT NULL,
    "used_at" datetime_text,
    "created_at" datetime_text NOT NULL,
    PRIMARY KEY ("id"),
    FOREIGN KEY ("user_id") REFERENCES "users" ("id") ON DELETE CASCADE
);

CREATE INDEX IF NOT EXISTS "index_user_recovery_codes_by_user_id" ON "user_recovery_codes" ("user_id");
