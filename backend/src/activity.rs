//! The activity feed: handlers record what happened, the feed endpoint reads it back.
//!
//! Recording is best effort. It happens after the change itself succeeded, and a failure to write
//! the entry is logged rather than failing the request the user actually made.

use serde_json::Value;

use crate::auth::now_secs;
use crate::models::Activity;

/// Everything one entry needs. `project_id`/`task_id` scope it to the project and task feeds.
pub struct Event {
    pub workspace_id: uuid::Uuid,
    pub project_id: Option<uuid::Uuid>,
    pub task_id: Option<uuid::Uuid>,
    pub actor_id: uuid::Uuid,
    pub kind: &'static str,
    pub data: Value,
}

pub async fn record(db: &mut toasty::Db, event: Event) {
    let created = toasty::create!(Activity {
        workspace_id: event.workspace_id,
        project_id: event.project_id,
        task_id: event.task_id,
        actor_id: event.actor_id,
        kind: event.kind.to_string(),
        data: event.data.to_string(),
        created_at: now_secs(),
    })
    .exec(db)
    .await;
    if let Err(e) = created {
        tracing::error!(error = %e, kind = event.kind, "recording activity failed");
    }
}
