//! Live editing endpoints: a session description (REST) and the WebSocket the editors share a page over.

use axum::Json;
use axum::extract::ws::{CloseFrame, Message as Ws, WebSocketUpgrade};
use axum::extract::{Path, Query, State};
use axum::response::Response;
use serde::{Deserialize, Serialize};
use utoipa::{IntoParams, ToSchema};
use utoipa_axum::{router::OpenApiRouter, routes};

use crate::access;
use crate::auth::CurrentUser;
use crate::error::ApiError;
use crate::live::{self, close};
use crate::models::Role;
use crate::state::AppState;

pub fn router() -> OpenApiRouter<AppState> {
    OpenApiRouter::new()
        .routes(routes!(live_session))
        .routes(routes!(live_socket))
}

#[derive(Serialize, ToSchema)]
pub struct LiveSession {
    /// Identifies the lineage of the page's live document. Pass it to the WebSocket; if the document is
    /// rebuilt (a restore, or a save without live sync) it changes and the socket closes with code 4001.
    pub epoch: String,
    /// Whether this person's edits are accepted (otherwise the connection is view-only).
    pub can_edit: bool,
}

/// Start live editing of a page: returns the document's epoch for the WebSocket.
#[utoipa::path(
    get,
    path = "/pages/{id}/live-session",
    tag = "wiki",
    security(("session_cookie" = [])),
    params(("id" = uuid::Uuid, Path, description = "Page id")),
    responses(
        (status = 200, description = "How to connect", body = LiveSession),
        (status = 404, description = "No such page, or no access")
    )
)]
pub async fn live_session(
    State(state): State<AppState>,
    CurrentUser { user, .. }: CurrentUser,
    Path(id): Path<uuid::Uuid>,
) -> Result<Json<LiveSession>, ApiError> {
    let can_edit = {
        let db = state.db();
        let page = access::require_page_access(db, &user, id, Role::Read).await?;
        access::role_in(db, &user, page.workspace_id)
            .await?
            .is_some_and(|role| role >= Role::Write)
    };
    let room = state.rooms.room(&state, id).await?;
    Ok(Json(LiveSession {
        epoch: room.epoch.clone(),
        can_edit,
    }))
}

#[derive(Deserialize, IntoParams)]
#[into_params(parameter_in = Query)]
pub struct LiveQuery {
    /// The epoch from the live session.
    pub epoch: String,
}

/// The live editing WebSocket (y-websocket protocol). Closes with 4001 when the epoch is stale or the
/// document was rebuilt, 4404 when the page was deleted, 4403 when access ended.
#[utoipa::path(
    get,
    path = "/pages/{id}/live",
    tag = "wiki",
    security(("session_cookie" = [])),
    params(("id" = uuid::Uuid, Path, description = "Page id"), LiveQuery),
    responses(
        (status = 101, description = "Switching to the WebSocket protocol"),
        (status = 404, description = "No such page, or no access")
    )
)]
pub async fn live_socket(
    State(state): State<AppState>,
    CurrentUser { user, .. }: CurrentUser,
    Path(id): Path<uuid::Uuid>,
    Query(query): Query<LiveQuery>,
    upgrade: WebSocketUpgrade,
) -> Result<Response, ApiError> {
    // The session cookie is SameSite=Lax, so a page on another site cannot open this with the user's login.
    let can_edit = {
        let db = state.db();
        let page = access::require_page_access(db, &user, id, Role::Read).await?;
        access::role_in(db, &user, page.workspace_id)
            .await?
            .is_some_and(|role| role >= Role::Write)
    };
    let room = state.rooms.room(&state, id).await?;
    Ok(upgrade.on_upgrade(move |mut socket| async move {
        if room.epoch != query.epoch {
            let _ = socket
                .send(Ws::Close(Some(CloseFrame {
                    code: close::RESET,
                    reason: "".into(),
                })))
                .await;
            return;
        }
        live::serve(socket, state, room, user.id, can_edit).await;
    }))
}
