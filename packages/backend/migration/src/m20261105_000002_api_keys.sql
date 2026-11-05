-- API keys: a person's token for scripts and integrations, sent as `Authorization: Bearer wm_...`. Only a
-- SHA-256 hash of the token is kept (it is 256 random bits, so a slow hash would add nothing); `prefix` is
-- its start, shown in lists so people can tell their keys apart. `scope` caps what the key may do, below the
-- person's own role. Revoking sets `revoked_at` (keys are never deleted, like calendar links).
CREATE TABLE IF NOT EXISTS "api_keys" (
    "id" uuid_text NOT NULL,
    "user_id" uuid_text NOT NULL,
    "name" TEXT NOT NULL,
    "prefix" TEXT NOT NULL,
    "token_hash" TEXT NOT NULL,
    "scope" TEXT NOT NULL CHECK ("scope" IN ('read', 'write', 'admin')),
    "created_at" datetime_text NOT NULL,
    "last_used_at" datetime_text,
    "expires_at" datetime_text,
    "revoked_at" datetime_text,
    PRIMARY KEY ("id"),
    FOREIGN KEY ("user_id") REFERENCES "users" ("id") ON DELETE CASCADE
);

CREATE UNIQUE INDEX IF NOT EXISTS "index_api_keys_by_token_hash" ON "api_keys" ("token_hash");

CREATE INDEX IF NOT EXISTS "index_api_keys_by_user_id" ON "api_keys" ("user_id");
