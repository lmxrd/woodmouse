-- Single sign-on (OpenID Connect): a user may have no password, accounts are linked to an identity at a
-- provider, and the provider settings the super admin edits.
--
-- `users.password_hash` becomes nullable (NULL: no password; the empty string used to mean that). SQLite
-- cannot alter a column, so the table is rebuilt. This file runs on one connection with `foreign_keys` OFF
-- (see the migration's `.rs`), because with it on, dropping `users` would cascade and delete every session,
-- membership and invite. The ids are UUIDs, so there is no `sqlite_sequence` row to carry over.
--
-- The migration is recorded only after it ran, so a crash in between runs it again: every step can repeat.

CREATE TABLE IF NOT EXISTS "users_new" (
    "id" uuid_text NOT NULL,
    "email" TEXT NOT NULL,
    "name" TEXT NOT NULL,
    "date_format" TEXT,
    "time_format" TEXT,
    "week_start" TEXT,
    "theme_mode" TEXT,
    "timezone" TEXT,
    "password_hash" TEXT,
    "super_admin" BOOLEAN NOT NULL,
    "global_role" TEXT CHECK ("global_role" IN ('read', 'write', 'admin')),
    "active" BOOLEAN NOT NULL,
    PRIMARY KEY ("id")
);

INSERT OR IGNORE INTO "users_new" (
    "id", "email", "name", "date_format", "time_format", "week_start", "theme_mode", "timezone",
    "password_hash", "super_admin", "global_role", "active"
)
SELECT
    "id", "email", "name", "date_format", "time_format", "week_start", "theme_mode", "timezone",
    NULLIF("password_hash", ''), "super_admin", "global_role", "active"
FROM "users";

DROP TABLE "users";

ALTER TABLE "users_new" RENAME TO "users";

CREATE UNIQUE INDEX IF NOT EXISTS "index_users_by_email" ON "users" ("email");

-- An account at an OpenID Connect provider (`iss` + `sub`) linked to a user. A user has at most one per
-- issuer, so a second account at the same provider cannot take over someone by email.
CREATE TABLE IF NOT EXISTS "user_identities" (
    "id" uuid_text NOT NULL,
    "user_id" uuid_text NOT NULL,
    "issuer" TEXT NOT NULL,
    "subject" TEXT NOT NULL,
    "created_at" datetime_text NOT NULL,
    PRIMARY KEY ("id"),
    FOREIGN KEY ("user_id") REFERENCES "users" ("id") ON DELETE CASCADE
);

CREATE UNIQUE INDEX IF NOT EXISTS "index_user_identities_by_issuer_and_subject" ON "user_identities" ("issuer", "subject");

CREATE UNIQUE INDEX IF NOT EXISTS "index_user_identities_by_user_id_and_issuer" ON "user_identities" ("user_id", "issuer");

-- The provider settings the super admin edits. An `OIDC_*` environment variable wins over its field. One
-- row, id 1.
CREATE TABLE IF NOT EXISTS "oidc_settings" (
    "id" BIGINT NOT NULL,
    "enabled" BOOLEAN NOT NULL,
    "issuer" TEXT,
    "client_id" TEXT,
    "client_secret" TEXT,
    "display_name" TEXT,
    "redirect_url" TEXT,
    "auto_create" BOOLEAN NOT NULL,
    PRIMARY KEY ("id")
);
