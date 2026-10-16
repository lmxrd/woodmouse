CREATE TABLE "labels" (
    "id" BLOB NOT NULL,
    "workspace_id" BLOB NOT NULL,
    "name" TEXT NOT NULL,
    "color" TEXT NOT NULL,
    "deleted_at" TEXT,
    PRIMARY KEY ("id")
);
-- #[toasty::breakpoint]
CREATE INDEX "index_labels_by_workspace_id" ON "labels" ("workspace_id");
-- #[toasty::breakpoint]
CREATE TABLE "calendar_links" (
    "id" BLOB NOT NULL,
    "user_id" BLOB NOT NULL,
    "scope" TEXT NOT NULL,
    "workspace_id" BLOB,
    "project_id" BLOB,
    "name" TEXT NOT NULL,
    "mode" TEXT NOT NULL,
    "secret" TEXT NOT NULL,
    "created_at" TEXT NOT NULL,
    "last_used_at" TEXT,
    "revoked_at" TEXT,
    PRIMARY KEY ("id")
);
-- #[toasty::breakpoint]
CREATE INDEX "index_calendar_links_by_user_id" ON "calendar_links" ("user_id");
-- #[toasty::breakpoint]
CREATE UNIQUE INDEX "index_calendar_links_by_secret" ON "calendar_links" ("secret");
-- #[toasty::breakpoint]
CREATE TABLE "users" (
    "id" BLOB NOT NULL,
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
-- #[toasty::breakpoint]
CREATE UNIQUE INDEX "index_users_by_email" ON "users" ("email");
-- #[toasty::breakpoint]
CREATE TABLE "content_links" (
    "id" BLOB NOT NULL,
    "source_page_id" BLOB NOT NULL,
    "target_id" BLOB NOT NULL,
    "target_kind" TEXT NOT NULL,
    PRIMARY KEY ("id")
);
-- #[toasty::breakpoint]
CREATE INDEX "index_content_links_by_source_page_id" ON "content_links" ("source_page_id");
-- #[toasty::breakpoint]
CREATE INDEX "index_content_links_by_target_id" ON "content_links" ("target_id");
-- #[toasty::breakpoint]
CREATE TABLE "lists" (
    "id" BLOB NOT NULL,
    "project_id" BLOB NOT NULL,
    "name" TEXT NOT NULL,
    "position" BIGINT NOT NULL,
    "deleted_at" TEXT,
    PRIMARY KEY ("id")
);
-- #[toasty::breakpoint]
CREATE INDEX "index_lists_by_project_id" ON "lists" ("project_id");
-- #[toasty::breakpoint]
CREATE TABLE "file_refs" (
    "file_id" BLOB NOT NULL,
    "target_type" TEXT NOT NULL,
    "target_id" BLOB NOT NULL,
    "embedded" BOOLEAN NOT NULL,
    PRIMARY KEY ("file_id", "target_type", "target_id")
);
-- #[toasty::breakpoint]
CREATE INDEX "index_file_refs_by_file_id" ON "file_refs" ("file_id");
-- #[toasty::breakpoint]
CREATE INDEX "index_file_refs_by_target_id" ON "file_refs" ("target_id");
-- #[toasty::breakpoint]
CREATE TABLE "task_labels" (
    "task_id" BLOB NOT NULL,
    "label_id" BLOB NOT NULL,
    PRIMARY KEY ("task_id", "label_id")
);
-- #[toasty::breakpoint]
CREATE INDEX "index_task_labels_by_task_id" ON "task_labels" ("task_id");
-- #[toasty::breakpoint]
CREATE INDEX "index_task_labels_by_label_id" ON "task_labels" ("label_id");
-- #[toasty::breakpoint]
CREATE TABLE "workspaces" (
    "id" BLOB NOT NULL,
    "name" TEXT NOT NULL,
    "description" TEXT NOT NULL,
    "theme_palette" TEXT,
    "theme_accent" TEXT,
    "deleted_at" TEXT,
    PRIMARY KEY ("id")
);
-- #[toasty::breakpoint]
CREATE TABLE "task_links" (
    "id" BLOB NOT NULL,
    "task_id" BLOB NOT NULL,
    "related_task_id" BLOB NOT NULL,
    "kind" TEXT NOT NULL CHECK ("kind" IN ('blocks', 'relates_to', 'duplicates')),
    PRIMARY KEY ("id")
);
-- #[toasty::breakpoint]
CREATE INDEX "index_task_links_by_task_id" ON "task_links" ("task_id");
-- #[toasty::breakpoint]
CREATE INDEX "index_task_links_by_related_task_id" ON "task_links" ("related_task_id");
-- #[toasty::breakpoint]
CREATE TABLE "saved_views" (
    "id" BLOB NOT NULL,
    "workspace_id" BLOB NOT NULL,
    "project_id" BLOB NOT NULL,
    "owner_id" BLOB NOT NULL,
    "name" TEXT NOT NULL,
    "params" TEXT NOT NULL,
    "is_shared" BOOLEAN NOT NULL,
    "created_at" TEXT NOT NULL,
    "deleted_at" TEXT,
    PRIMARY KEY ("id")
);
-- #[toasty::breakpoint]
CREATE INDEX "index_saved_views_by_workspace_id" ON "saved_views" ("workspace_id");
-- #[toasty::breakpoint]
CREATE INDEX "index_saved_views_by_project_id" ON "saved_views" ("project_id");
-- #[toasty::breakpoint]
CREATE TABLE "comments" (
    "id" BLOB NOT NULL,
    "task_id" BLOB NOT NULL,
    "author_id" BLOB NOT NULL,
    "body" TEXT NOT NULL,
    "created_at" TEXT NOT NULL,
    "deleted_at" TEXT,
    PRIMARY KEY ("id")
);
-- #[toasty::breakpoint]
CREATE INDEX "index_comments_by_task_id" ON "comments" ("task_id");
-- #[toasty::breakpoint]
CREATE TABLE "tasks" (
    "id" BLOB NOT NULL,
    "project_id" BLOB NOT NULL,
    "list_id" BLOB,
    "title" TEXT NOT NULL,
    "description" TEXT NOT NULL,
    "completed_at" TEXT,
    "position" BIGINT NOT NULL,
    "priority" TEXT NOT NULL CHECK ("priority" IN ('low', 'medium', 'high', 'urgent')),
    "start_date" TEXT,
    "due_date" TEXT,
    "estimate_minutes" BIGINT,
    "spent_minutes" BIGINT,
    "parent_id" BLOB,
    "deleted_at" TEXT,
    PRIMARY KEY ("id")
);
-- #[toasty::breakpoint]
CREATE INDEX "index_tasks_by_project_id" ON "tasks" ("project_id");
-- #[toasty::breakpoint]
CREATE INDEX "index_tasks_by_list_id" ON "tasks" ("list_id");
-- #[toasty::breakpoint]
CREATE INDEX "index_tasks_by_parent_id" ON "tasks" ("parent_id");
-- #[toasty::breakpoint]
CREATE TABLE "task_assignees" (
    "task_id" BLOB NOT NULL,
    "user_id" BLOB NOT NULL,
    PRIMARY KEY ("task_id", "user_id")
);
-- #[toasty::breakpoint]
CREATE INDEX "index_task_assignees_by_task_id" ON "task_assignees" ("task_id");
-- #[toasty::breakpoint]
CREATE INDEX "index_task_assignees_by_user_id" ON "task_assignees" ("user_id");
-- #[toasty::breakpoint]
CREATE TABLE "files" (
    "id" BLOB NOT NULL,
    "workspace_id" BLOB NOT NULL,
    "name" TEXT NOT NULL,
    "mime" TEXT NOT NULL,
    "size" BIGINT NOT NULL,
    "sha256" TEXT NOT NULL,
    "storage_backend" TEXT NOT NULL,
    "storage_key" TEXT NOT NULL,
    "uploaded_by" BLOB NOT NULL,
    "created_at" TEXT NOT NULL,
    PRIMARY KEY ("id")
);
-- #[toasty::breakpoint]
CREATE INDEX "index_files_by_workspace_id" ON "files" ("workspace_id");
-- #[toasty::breakpoint]
CREATE INDEX "index_files_by_sha256" ON "files" ("sha256");
-- #[toasty::breakpoint]
CREATE TABLE "user_invites" (
    "id" BLOB NOT NULL,
    "token" TEXT NOT NULL,
    "user_id" BLOB NOT NULL,
    "expires_at" TEXT NOT NULL,
    PRIMARY KEY ("id")
);
-- #[toasty::breakpoint]
CREATE UNIQUE INDEX "index_user_invites_by_token" ON "user_invites" ("token");
-- #[toasty::breakpoint]
CREATE INDEX "index_user_invites_by_user_id" ON "user_invites" ("user_id");
-- #[toasty::breakpoint]
CREATE TABLE "revisions" (
    "id" BLOB NOT NULL,
    "target_type" TEXT NOT NULL,
    "target_id" BLOB NOT NULL,
    "actor_id" BLOB NOT NULL,
    "created_at" TEXT NOT NULL,
    "updated_at" TEXT NOT NULL,
    "content" TEXT NOT NULL,
    "redacted_at" TEXT,
    PRIMARY KEY ("id")
);
-- #[toasty::breakpoint]
CREATE INDEX "index_revisions_by_target_id" ON "revisions" ("target_id");
-- #[toasty::breakpoint]
CREATE TABLE "page_docs" (
    "page_id" BLOB NOT NULL,
    "epoch" TEXT NOT NULL,
    "state" TEXT NOT NULL,
    "updated_at" TEXT NOT NULL,
    PRIMARY KEY ("page_id")
);
-- #[toasty::breakpoint]
CREATE TABLE "sessions" (
    "id" BLOB NOT NULL,
    "token" TEXT NOT NULL,
    "user_id" BLOB NOT NULL,
    "expires_at" TEXT NOT NULL,
    PRIMARY KEY ("id")
);
-- #[toasty::breakpoint]
CREATE UNIQUE INDEX "index_sessions_by_token" ON "sessions" ("token");
-- #[toasty::breakpoint]
CREATE INDEX "index_sessions_by_user_id" ON "sessions" ("user_id");
-- #[toasty::breakpoint]
CREATE TABLE "audit_logs" (
    "id" BLOB NOT NULL,
    "actor_id" BLOB,
    "action" TEXT NOT NULL,
    "target_type" TEXT,
    "target_id" BLOB,
    "workspace_id" BLOB,
    "visibility" TEXT NOT NULL CHECK ("visibility" IN ('workspace', 'admin')),
    "data" TEXT NOT NULL,
    "created_at" TEXT NOT NULL,
    PRIMARY KEY ("id")
);
-- #[toasty::breakpoint]
CREATE INDEX "index_audit_logs_by_workspace_id_and_visibility_and_created_at" ON "audit_logs" ("workspace_id", "visibility", "created_at");
-- #[toasty::breakpoint]
CREATE INDEX "index_audit_logs_by_actor_id" ON "audit_logs" ("actor_id");
-- #[toasty::breakpoint]
CREATE INDEX "index_audit_logs_by_action" ON "audit_logs" ("action");
-- #[toasty::breakpoint]
CREATE INDEX "index_audit_logs_by_target_id" ON "audit_logs" ("target_id");
-- #[toasty::breakpoint]
CREATE TABLE "projects" (
    "id" BLOB NOT NULL,
    "workspace_id" BLOB NOT NULL,
    "name" TEXT NOT NULL,
    "description" TEXT NOT NULL,
    "deleted_at" TEXT,
    PRIMARY KEY ("id")
);
-- #[toasty::breakpoint]
CREATE INDEX "index_projects_by_workspace_id" ON "projects" ("workspace_id");
-- #[toasty::breakpoint]
CREATE TABLE "wiki_pages" (
    "id" BLOB NOT NULL,
    "workspace_id" BLOB NOT NULL,
    "parent_id" BLOB,
    "title" TEXT NOT NULL,
    "position" BIGINT NOT NULL,
    "content" TEXT NOT NULL,
    "created_by" BLOB NOT NULL,
    "created_at" TEXT NOT NULL,
    "updated_by" BLOB NOT NULL,
    "updated_at" TEXT NOT NULL,
    "version" BIGINT NOT NULL,
    "deleted_at" TEXT,
    PRIMARY KEY ("id")
);
-- #[toasty::breakpoint]
CREATE INDEX "index_wiki_pages_by_workspace_id" ON "wiki_pages" ("workspace_id");
-- #[toasty::breakpoint]
CREATE INDEX "index_wiki_pages_by_parent_id" ON "wiki_pages" ("parent_id");
-- #[toasty::breakpoint]
CREATE TABLE "workspace_members" (
    "user_id" BLOB NOT NULL,
    "workspace_id" BLOB NOT NULL,
    "role" TEXT NOT NULL CHECK ("role" IN ('read', 'write', 'admin')),
    "theme_palette" TEXT,
    "theme_accent" TEXT,
    PRIMARY KEY ("user_id", "workspace_id")
);
-- #[toasty::breakpoint]
CREATE INDEX "index_workspace_members_by_user_id" ON "workspace_members" ("user_id");