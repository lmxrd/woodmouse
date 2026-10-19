-- The whole schema. The column type names are the ones SeaORM uses for SQLite (so `sea-orm-cli` reads them back as
-- the right Rust types): `uuid_text` holds a UUID, stored as its 16 bytes (the name is SeaORM's, the bytes are what
-- sqlx and SeaORM write); `datetime_text` holds a UTC date-time as `YYYY-MM-DDTHH:MM:SS` text. Enums are their
-- lowercase labels, JSON is TEXT. Relations are foreign keys (enforced: `foreign_keys` is on):
-- the join and child rows of a user, workspace, task, file or page go with it (ON DELETE CASCADE), and a row that
-- still has children cannot be deleted. Polymorphic targets (`file_refs.target_id`, `revisions.target_id`,
-- `content_links.target_id`, `watches.target_id`, audit log ids) are not foreign keys.

CREATE TABLE "workspaces" (
    "id" uuid_text NOT NULL,
    "name" TEXT NOT NULL,
    "description" TEXT NOT NULL,
    "theme_palette" TEXT,
    "theme_accent" TEXT,
    "deleted_at" datetime_text,
    PRIMARY KEY ("id")
);

CREATE TABLE "notifications" (
    "id" uuid_text NOT NULL,
    "user_id" uuid_text NOT NULL,
    "kind" TEXT NOT NULL,
    "workspace_id" uuid_text NOT NULL,
    "target_type" TEXT,
    "target_id" uuid_text,
    "actor_id" uuid_text,
    "data" TEXT NOT NULL,
    "created_at" datetime_text NOT NULL,
    "read_at" datetime_text,
    "emailed_at" datetime_text,
    PRIMARY KEY ("id"),
    FOREIGN KEY ("user_id") REFERENCES "users" ("id") ON DELETE CASCADE,
    FOREIGN KEY ("workspace_id") REFERENCES "workspaces" ("id"),
    FOREIGN KEY ("actor_id") REFERENCES "users" ("id")
);

CREATE INDEX "index_notifications_by_user_id_and_created_at" ON "notifications" ("user_id", "created_at");

CREATE TABLE "wiki_pages" (
    "id" uuid_text NOT NULL,
    "workspace_id" uuid_text NOT NULL,
    "parent_id" uuid_text,
    "title" TEXT NOT NULL,
    "position" BIGINT NOT NULL,
    "content" TEXT NOT NULL,
    "created_by" uuid_text NOT NULL,
    "created_at" datetime_text NOT NULL,
    "updated_by" uuid_text NOT NULL,
    "updated_at" datetime_text NOT NULL,
    "version" BIGINT NOT NULL,
    "deleted_at" datetime_text,
    PRIMARY KEY ("id"),
    FOREIGN KEY ("workspace_id") REFERENCES "workspaces" ("id"),
    FOREIGN KEY ("parent_id") REFERENCES "wiki_pages" ("id"),
    FOREIGN KEY ("created_by") REFERENCES "users" ("id"),
    FOREIGN KEY ("updated_by") REFERENCES "users" ("id")
);

CREATE INDEX "index_wiki_pages_by_workspace_id" ON "wiki_pages" ("workspace_id");

CREATE INDEX "index_wiki_pages_by_parent_id" ON "wiki_pages" ("parent_id");

CREATE TABLE "revisions" (
    "id" uuid_text NOT NULL,
    "target_type" TEXT NOT NULL,
    "target_id" uuid_text NOT NULL,
    "actor_id" uuid_text NOT NULL,
    "created_at" datetime_text NOT NULL,
    "updated_at" datetime_text NOT NULL,
    "content" TEXT NOT NULL,
    "redacted_at" datetime_text,
    PRIMARY KEY ("id"),
    FOREIGN KEY ("actor_id") REFERENCES "users" ("id")
);

CREATE INDEX "index_revisions_by_target_id" ON "revisions" ("target_id");

CREATE TABLE "content_links" (
    "id" uuid_text NOT NULL,
    "source_page_id" uuid_text NOT NULL,
    "target_id" uuid_text NOT NULL,
    "target_kind" TEXT NOT NULL,
    PRIMARY KEY ("id"),
    FOREIGN KEY ("source_page_id") REFERENCES "wiki_pages" ("id") ON DELETE CASCADE
);

CREATE INDEX "index_content_links_by_source_page_id" ON "content_links" ("source_page_id");

CREATE INDEX "index_content_links_by_target_id" ON "content_links" ("target_id");

CREATE TABLE "file_refs" (
"file_id" uuid_text NOT NULL,
    "target_type" TEXT NOT NULL,
    "target_id" uuid_text NOT NULL,
    "embedded" BOOLEAN NOT NULL,
    PRIMARY KEY ("file_id", "target_type", "target_id"),
    FOREIGN KEY ("file_id") REFERENCES "files" ("id") ON DELETE CASCADE
);

CREATE INDEX "index_file_refs_by_file_id" ON "file_refs" ("file_id");

CREATE INDEX "index_file_refs_by_target_id" ON "file_refs" ("target_id");

CREATE TABLE "labels" (
    "id" uuid_text NOT NULL,
    "workspace_id" uuid_text NOT NULL,
    "name" TEXT NOT NULL,
    "color" TEXT NOT NULL,
    "deleted_at" datetime_text,
    PRIMARY KEY ("id"),
    FOREIGN KEY ("workspace_id") REFERENCES "workspaces" ("id")
);

CREATE INDEX "index_labels_by_workspace_id" ON "labels" ("workspace_id");

CREATE TABLE "watches" (
"user_id" uuid_text NOT NULL,
    "target_type" TEXT NOT NULL,
    "target_id" uuid_text NOT NULL,
    "created_at" datetime_text NOT NULL,
    PRIMARY KEY ("user_id", "target_type", "target_id"),
    FOREIGN KEY ("user_id") REFERENCES "users" ("id") ON DELETE CASCADE
);

CREATE INDEX "index_watches_by_user_id" ON "watches" ("user_id");

CREATE INDEX "index_watches_by_target_id" ON "watches" ("target_id");

CREATE TABLE "email_settings" (
    "id" BIGINT NOT NULL,
    "host" TEXT,
    "port" BIGINT,
    "username" TEXT,
    "password" TEXT,
    "sender" TEXT,
    "tls" TEXT,
    PRIMARY KEY ("id")
);

CREATE TABLE "calendar_links" (
    "id" uuid_text NOT NULL,
    "user_id" uuid_text NOT NULL,
    "scope" TEXT NOT NULL,
    "workspace_id" uuid_text,
    "project_id" uuid_text,
    "name" TEXT NOT NULL,
    "mode" TEXT NOT NULL,
    "secret" TEXT NOT NULL,
    "created_at" datetime_text NOT NULL,
    "last_used_at" datetime_text,
    "revoked_at" datetime_text,
    PRIMARY KEY ("id"),
    FOREIGN KEY ("user_id") REFERENCES "users" ("id") ON DELETE CASCADE,
    FOREIGN KEY ("workspace_id") REFERENCES "workspaces" ("id"),
    FOREIGN KEY ("project_id") REFERENCES "projects" ("id")
);

CREATE INDEX "index_calendar_links_by_user_id" ON "calendar_links" ("user_id");

CREATE UNIQUE INDEX "index_calendar_links_by_secret" ON "calendar_links" ("secret");

CREATE TABLE "lists" (
    "id" uuid_text NOT NULL,
    "project_id" uuid_text NOT NULL,
    "name" TEXT NOT NULL,
    "position" BIGINT NOT NULL,
    "deleted_at" datetime_text,
    PRIMARY KEY ("id"),
    FOREIGN KEY ("project_id") REFERENCES "projects" ("id")
);

CREATE INDEX "index_lists_by_project_id" ON "lists" ("project_id");

CREATE TABLE "task_assignees" (
"task_id" uuid_text NOT NULL,
    "user_id" uuid_text NOT NULL,
    PRIMARY KEY ("task_id", "user_id"),
    FOREIGN KEY ("task_id") REFERENCES "tasks" ("id") ON DELETE CASCADE,
    FOREIGN KEY ("user_id") REFERENCES "users" ("id") ON DELETE CASCADE
);

CREATE INDEX "index_task_assignees_by_task_id" ON "task_assignees" ("task_id");

CREATE INDEX "index_task_assignees_by_user_id" ON "task_assignees" ("user_id");

CREATE TABLE "users" (
    "id" uuid_text NOT NULL,
    "email" TEXT NOT NULL,
    "name" TEXT NOT NULL,
    "date_format" TEXT,
    "time_format" TEXT,
    "week_start" TEXT,
    "theme_mode" TEXT,
    "timezone" TEXT,
    "password_hash" TEXT NOT NULL,
    "super_admin" BOOLEAN NOT NULL,
    "global_role" TEXT CHECK ("global_role" IN ('read', 'write', 'admin')),
    "active" BOOLEAN NOT NULL,
    PRIMARY KEY ("id")
);

CREATE UNIQUE INDEX "index_users_by_email" ON "users" ("email");

CREATE TABLE "notification_settings" (
"user_id" uuid_text NOT NULL,
    "email_instant" BOOLEAN NOT NULL,
    "digest" BOOLEAN NOT NULL,
    "digest_hour" BIGINT NOT NULL,
    "last_digest_at" datetime_text,
    PRIMARY KEY ("user_id"),
    FOREIGN KEY ("user_id") REFERENCES "users" ("id") ON DELETE CASCADE
);

CREATE TABLE "workspace_members" (
"user_id" uuid_text NOT NULL,
    "workspace_id" uuid_text NOT NULL,
    "role" TEXT NOT NULL CHECK ("role" IN ('read', 'write', 'admin')),
    "theme_palette" TEXT,
    "theme_accent" TEXT,
    PRIMARY KEY ("user_id", "workspace_id"),
    FOREIGN KEY ("user_id") REFERENCES "users" ("id") ON DELETE CASCADE,
    FOREIGN KEY ("workspace_id") REFERENCES "workspaces" ("id") ON DELETE CASCADE
);

CREATE INDEX "index_workspace_members_by_user_id" ON "workspace_members" ("user_id");

CREATE TABLE "page_docs" (
"page_id" uuid_text NOT NULL,
    "epoch" TEXT NOT NULL,
    "state" TEXT NOT NULL,
    "updated_at" datetime_text NOT NULL,
    PRIMARY KEY ("page_id"),
    FOREIGN KEY ("page_id") REFERENCES "wiki_pages" ("id") ON DELETE CASCADE
);

CREATE TABLE "file_versions" (
    "id" uuid_text NOT NULL,
    "file_id" uuid_text NOT NULL,
    "number" BIGINT NOT NULL,
    "name" TEXT NOT NULL,
    "mime" TEXT NOT NULL,
    "size" BIGINT NOT NULL,
    "sha256" TEXT NOT NULL,
    "storage_backend" TEXT NOT NULL,
    "storage_key" TEXT NOT NULL,
    "uploaded_by" uuid_text NOT NULL,
    "created_at" datetime_text NOT NULL,
    PRIMARY KEY ("id"),
    FOREIGN KEY ("file_id") REFERENCES "files" ("id") ON DELETE CASCADE,
    FOREIGN KEY ("uploaded_by") REFERENCES "users" ("id")
);

CREATE INDEX "index_file_versions_by_file_id" ON "file_versions" ("file_id");

CREATE TABLE "user_invites" (
    "id" uuid_text NOT NULL,
    "token" TEXT NOT NULL,
    "user_id" uuid_text NOT NULL,
    "expires_at" datetime_text NOT NULL,
    PRIMARY KEY ("id"),
    FOREIGN KEY ("user_id") REFERENCES "users" ("id") ON DELETE CASCADE
);

CREATE UNIQUE INDEX "index_user_invites_by_token" ON "user_invites" ("token");

CREATE INDEX "index_user_invites_by_user_id" ON "user_invites" ("user_id");

CREATE TABLE "sessions" (
    "id" uuid_text NOT NULL,
    "token" TEXT NOT NULL,
    "user_id" uuid_text NOT NULL,
    "expires_at" datetime_text NOT NULL,
    PRIMARY KEY ("id"),
    FOREIGN KEY ("user_id") REFERENCES "users" ("id") ON DELETE CASCADE
);

CREATE UNIQUE INDEX "index_sessions_by_token" ON "sessions" ("token");

CREATE INDEX "index_sessions_by_user_id" ON "sessions" ("user_id");

CREATE TABLE "sprints" (
    "id" uuid_text NOT NULL,
    "project_id" uuid_text NOT NULL,
    "name" TEXT NOT NULL,
    "goal" TEXT NOT NULL,
    "start_date" datetime_text NOT NULL,
    "end_date" datetime_text NOT NULL,
    "completed_at" datetime_text,
    "deleted_at" datetime_text,
    PRIMARY KEY ("id"),
    FOREIGN KEY ("project_id") REFERENCES "projects" ("id")
);

CREATE INDEX "index_sprints_by_project_id" ON "sprints" ("project_id");

CREATE TABLE "saved_views" (
    "id" uuid_text NOT NULL,
    "workspace_id" uuid_text NOT NULL,
    "project_id" uuid_text NOT NULL,
    "owner_id" uuid_text NOT NULL,
    "name" TEXT NOT NULL,
    "params" TEXT NOT NULL,
    "is_shared" BOOLEAN NOT NULL,
    "created_at" datetime_text NOT NULL,
    "deleted_at" datetime_text,
    PRIMARY KEY ("id"),
    FOREIGN KEY ("workspace_id") REFERENCES "workspaces" ("id"),
    FOREIGN KEY ("project_id") REFERENCES "projects" ("id"),
    FOREIGN KEY ("owner_id") REFERENCES "users" ("id")
);

CREATE INDEX "index_saved_views_by_workspace_id" ON "saved_views" ("workspace_id");

CREATE INDEX "index_saved_views_by_project_id" ON "saved_views" ("project_id");

CREATE TABLE "files" (
    "id" uuid_text NOT NULL,
    "workspace_id" uuid_text NOT NULL,
    "name" TEXT NOT NULL,
    "mime" TEXT NOT NULL,
    "size" BIGINT NOT NULL,
    "sha256" TEXT NOT NULL,
    "storage_backend" TEXT NOT NULL,
    "storage_key" TEXT NOT NULL,
    "uploaded_by" uuid_text NOT NULL,
    "created_at" datetime_text NOT NULL,
    "deleted_at" datetime_text,
    PRIMARY KEY ("id"),
    FOREIGN KEY ("workspace_id") REFERENCES "workspaces" ("id"),
    FOREIGN KEY ("uploaded_by") REFERENCES "users" ("id")
);

CREATE INDEX "index_files_by_workspace_id" ON "files" ("workspace_id");

CREATE INDEX "index_files_by_sha256" ON "files" ("sha256");

CREATE TABLE "projects" (
    "id" uuid_text NOT NULL,
    "workspace_id" uuid_text NOT NULL,
    "name" TEXT NOT NULL,
    "description" TEXT NOT NULL,
    "deleted_at" datetime_text,
    PRIMARY KEY ("id"),
    FOREIGN KEY ("workspace_id") REFERENCES "workspaces" ("id")
);

CREATE INDEX "index_projects_by_workspace_id" ON "projects" ("workspace_id");

CREATE TABLE "audit_logs" (
    "id" uuid_text NOT NULL,
    "actor_id" uuid_text,
    "action" TEXT NOT NULL,
    "target_type" TEXT,
    "target_id" uuid_text,
    "workspace_id" uuid_text,
    "visibility" TEXT NOT NULL CHECK ("visibility" IN ('workspace', 'admin')),
    "data" TEXT NOT NULL,
    "created_at" datetime_text NOT NULL,
    PRIMARY KEY ("id")
);

CREATE INDEX "index_audit_logs_by_workspace_id_and_visibility_and_created_at" ON "audit_logs" ("workspace_id", "visibility", "created_at");

CREATE INDEX "index_audit_logs_by_actor_id" ON "audit_logs" ("actor_id");

CREATE INDEX "index_audit_logs_by_action" ON "audit_logs" ("action");

CREATE INDEX "index_audit_logs_by_target_id" ON "audit_logs" ("target_id");

CREATE TABLE "comments" (
    "id" uuid_text NOT NULL,
    "task_id" uuid_text NOT NULL,
    "author_id" uuid_text NOT NULL,
    "body" TEXT NOT NULL,
    "created_at" datetime_text NOT NULL,
    "deleted_at" datetime_text,
    PRIMARY KEY ("id"),
    FOREIGN KEY ("task_id") REFERENCES "tasks" ("id"),
    FOREIGN KEY ("author_id") REFERENCES "users" ("id")
);

CREATE INDEX "index_comments_by_task_id" ON "comments" ("task_id");

CREATE TABLE "tasks" (
    "id" uuid_text NOT NULL,
    "project_id" uuid_text NOT NULL,
    "list_id" uuid_text,
    "title" TEXT NOT NULL,
    "description" TEXT NOT NULL,
    "completed_at" datetime_text,
    "position" BIGINT NOT NULL,
    "priority" TEXT NOT NULL CHECK ("priority" IN ('low', 'medium', 'high', 'urgent')),
    "start_date" datetime_text,
    "due_date" datetime_text,
    "sprint_id" uuid_text,
    "estimate_minutes" BIGINT,
    "spent_minutes" BIGINT,
    "parent_id" uuid_text,
    "deleted_at" datetime_text,
    PRIMARY KEY ("id"),
    FOREIGN KEY ("project_id") REFERENCES "projects" ("id"),
    FOREIGN KEY ("list_id") REFERENCES "lists" ("id"),
    FOREIGN KEY ("sprint_id") REFERENCES "sprints" ("id"),
    FOREIGN KEY ("parent_id") REFERENCES "tasks" ("id")
);

CREATE INDEX "index_tasks_by_project_id" ON "tasks" ("project_id");

CREATE INDEX "index_tasks_by_list_id" ON "tasks" ("list_id");

CREATE INDEX "index_tasks_by_sprint_id" ON "tasks" ("sprint_id");

CREATE INDEX "index_tasks_by_parent_id" ON "tasks" ("parent_id");

CREATE TABLE "task_links" (
    "id" uuid_text NOT NULL,
    "task_id" uuid_text NOT NULL,
    "related_task_id" uuid_text NOT NULL,
    "kind" TEXT NOT NULL CHECK ("kind" IN ('blocks', 'relates_to', 'duplicates')),
    PRIMARY KEY ("id"),
    FOREIGN KEY ("task_id") REFERENCES "tasks" ("id") ON DELETE CASCADE,
    FOREIGN KEY ("related_task_id") REFERENCES "tasks" ("id") ON DELETE CASCADE
);

CREATE INDEX "index_task_links_by_task_id" ON "task_links" ("task_id");

CREATE INDEX "index_task_links_by_related_task_id" ON "task_links" ("related_task_id");

CREATE TABLE "task_labels" (
"task_id" uuid_text NOT NULL,
    "label_id" uuid_text NOT NULL,
    PRIMARY KEY ("task_id", "label_id"),
    FOREIGN KEY ("task_id") REFERENCES "tasks" ("id") ON DELETE CASCADE,
    FOREIGN KEY ("label_id") REFERENCES "labels" ("id") ON DELETE CASCADE
);

CREATE INDEX "index_task_labels_by_task_id" ON "task_labels" ("task_id");

CREATE INDEX "index_task_labels_by_label_id" ON "task_labels" ("label_id");

-- Full-text search over pages, tasks and comments (SQLite FTS5): one searchable row per item in
-- `search_docs`, an external-content FTS index over it, and triggers that keep both current. See
-- `src/content/search.rs`.

CREATE TABLE search_docs (
        rid INTEGER PRIMARY KEY,
        id TEXT NOT NULL UNIQUE,
        kind TEXT NOT NULL,
        ws TEXT NOT NULL,
        title TEXT NOT NULL,
        body TEXT NOT NULL DEFAULT ''
    );

CREATE INDEX search_docs_ws ON search_docs(ws);

CREATE VIRTUAL TABLE search_fts USING fts5(
        title, body, content='search_docs', content_rowid='rid',
        tokenize='unicode61 remove_diacritics 2', prefix='2 3'
    );

CREATE TRIGGER search_docs_ai AFTER INSERT ON search_docs BEGIN
        INSERT INTO search_fts(rowid, title, body) VALUES (new.rid, new.title, new.body);
    END;

CREATE TRIGGER search_docs_ad AFTER DELETE ON search_docs BEGIN
        INSERT INTO search_fts(search_fts, rowid, title, body)
            VALUES ('delete', old.rid, old.title, old.body);
    END;

CREATE TRIGGER search_docs_au AFTER UPDATE ON search_docs BEGIN
        INSERT INTO search_fts(search_fts, rowid, title, body)
            VALUES ('delete', old.rid, old.title, old.body);
        INSERT INTO search_fts(rowid, title, body) VALUES (new.rid, new.title, new.body);
    END;

CREATE TRIGGER search_tasks_ai AFTER INSERT ON tasks BEGIN
        INSERT INTO search_docs(id, kind, ws, title, body)
            VALUES (lower(hex(new.id)), 'task',
                    (SELECT lower(hex(workspace_id)) FROM projects WHERE id = new.project_id),
                    new.title, new.description)
            ON CONFLICT(id) DO UPDATE SET title = excluded.title, body = excluded.body;
    END;

CREATE TRIGGER search_tasks_au AFTER UPDATE OF title, description ON tasks BEGIN
        INSERT INTO search_docs(id, kind, ws, title, body)
            VALUES (lower(hex(new.id)), 'task',
                    (SELECT lower(hex(workspace_id)) FROM projects WHERE id = new.project_id),
                    new.title, new.description)
            ON CONFLICT(id) DO UPDATE SET title = excluded.title, body = excluded.body;
    END;

CREATE TRIGGER search_comments_ai AFTER INSERT ON comments BEGIN
        INSERT INTO search_docs(id, kind, ws, title, body)
            VALUES (lower(hex(new.id)), 'comment',
                    (SELECT lower(hex(p.workspace_id)) FROM tasks t JOIN projects p ON p.id = t.project_id
                      WHERE t.id = new.task_id),
                    '', new.body)
            ON CONFLICT(id) DO UPDATE SET body = excluded.body;
    END;

CREATE TRIGGER search_comments_au AFTER UPDATE OF body ON comments BEGIN
        UPDATE search_docs SET body = new.body WHERE id = lower(hex(new.id));
    END;

CREATE TRIGGER search_pages_ai AFTER INSERT ON wiki_pages BEGIN
        INSERT INTO search_docs(id, kind, ws, title)
            VALUES (lower(hex(new.id)), 'page', lower(hex(new.workspace_id)), new.title)
            ON CONFLICT(id) DO UPDATE SET title = excluded.title;
    END;

CREATE TRIGGER search_pages_au AFTER UPDATE OF title ON wiki_pages BEGIN
        UPDATE search_docs SET title = new.title WHERE id = lower(hex(new.id));
    END;
