//! The audit log: an append-only record of what happened, which is also the source of the workspace
//! activity feed. Entries are written in the same transaction as the change they describe (pass the
//! transaction as the executor), so a change and its record succeed or fail together. They are never
//! updated or deleted, and hold no content and no network addresses or browsers; edits point to the
//! `Revision` that holds the content.

use serde_json::{Value, json};

use crate::error::ApiError;
use crate::models::{AuditLog, Visibility};
use crate::time;

/// One entry to record.
pub struct Entry {
    actor_id: Option<uuid::Uuid>,
    action: &'static str,
    target_type: Option<&'static str>,
    target_id: Option<uuid::Uuid>,
    workspace_id: Option<uuid::Uuid>,
    visibility: Visibility,
    data: Value,
}

impl Entry {
    /// An event that appears in the workspace's activity feed (and the audit log).
    pub fn feed(workspace_id: uuid::Uuid, actor_id: uuid::Uuid, action: &'static str) -> Self {
        Self {
            actor_id: Some(actor_id),
            action,
            target_type: None,
            target_id: None,
            workspace_id: Some(workspace_id),
            visibility: Visibility::Workspace,
            data: json!({}),
        }
    }

    /// An event only the super admin sees in the audit log (sign-ins, privilege changes, ...).
    pub fn admin(actor_id: Option<uuid::Uuid>, action: &'static str) -> Self {
        Self {
            actor_id,
            action,
            target_type: None,
            target_id: None,
            workspace_id: None,
            visibility: Visibility::Admin,
            data: json!({}),
        }
    }

    /// What the entry is about.
    pub fn target(mut self, target_type: &'static str, target_id: uuid::Uuid) -> Self {
        self.target_type = Some(target_type);
        self.target_id = Some(target_id);
        self
    }

    /// The workspace an admin-only event belongs to (feed events always have one).
    pub fn in_workspace(mut self, workspace_id: uuid::Uuid) -> Self {
        self.workspace_id = Some(workspace_id);
        self
    }

    /// What the UI needs to phrase it (snapshotted titles, old and new values, ids). Never content.
    pub fn data(mut self, data: Value) -> Self {
        self.data = data;
        self
    }
}

/// Write the entry. Call it with the transaction of the change it records.
pub async fn record(db: &mut dyn toasty::Executor, entry: Entry) -> Result<(), ApiError> {
    debug_assert!(
        entry.visibility != Visibility::Workspace || entry.workspace_id.is_some(),
        "feed entries need a workspace"
    );
    toasty::create!(AuditLog {
        actor_id: entry.actor_id,
        action: entry.action.to_string(),
        target_type: entry.target_type.map(str::to_string),
        target_id: entry.target_id,
        workspace_id: entry.workspace_id,
        visibility: entry.visibility,
        data: entry.data.to_string(),
        created_at: time::now(),
    })
    .exec(db)
    .await?;
    Ok(())
}

/// Record a soft delete (audit log only; the Trash shows who deleted what from these).
pub async fn deleted(
    db: &mut dyn toasty::Executor,
    actor_id: uuid::Uuid,
    workspace_id: uuid::Uuid,
    target_type: &'static str,
    target_id: uuid::Uuid,
    title: &str,
) -> Result<(), ApiError> {
    record(
        db,
        Entry::admin(Some(actor_id), "entity_deleted")
            .target(target_type, target_id)
            .in_workspace(workspace_id)
            .data(json!({ "title": title })),
    )
    .await
}

/// Record a restore from the Trash.
pub async fn restored(
    db: &mut dyn toasty::Executor,
    actor_id: uuid::Uuid,
    workspace_id: uuid::Uuid,
    target_type: &'static str,
    target_id: uuid::Uuid,
    title: &str,
) -> Result<(), ApiError> {
    record(
        db,
        Entry::admin(Some(actor_id), "entity_restored")
            .target(target_type, target_id)
            .in_workspace(workspace_id)
            .data(json!({ "title": title })),
    )
    .await
}

/// Record an event about a task in the workspace's activity feed. `project_id` goes into the data so
/// the project's feed can be filtered.
#[allow(clippy::too_many_arguments)]
pub async fn task_event(
    db: &mut dyn toasty::Executor,
    workspace_id: uuid::Uuid,
    project_id: uuid::Uuid,
    actor_id: uuid::Uuid,
    action: &'static str,
    task_id: uuid::Uuid,
    mut data: Value,
) -> Result<(), ApiError> {
    if let Some(object) = data.as_object_mut() {
        object.insert("project_id".to_string(), json!(project_id));
    }
    record(
        db,
        Entry::feed(workspace_id, actor_id, action)
            .target("task", task_id)
            .data(data),
    )
    .await
}
