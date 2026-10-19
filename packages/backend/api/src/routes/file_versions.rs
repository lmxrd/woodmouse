//! File versions: uploading a new version of a library file keeps the old content, under the same file id
//! (so attachments and page blocks keep working). Deleting a file is soft (Trash).

use crate::db::Db;
use crate::db::prelude::*;
use crate::time::DateTime;
use axum::Json;
use axum::extract::{DefaultBodyLimit, Multipart, Path, State};
use axum::http::StatusCode;
use axum::response::Response;
use entity::users;
use entity::{file_versions, files};
use serde::Serialize;
use utoipa::ToSchema;
use utoipa_axum::{router::OpenApiRouter, routes};

use crate::access;
use crate::audit::{self, Entry};
use crate::auth::CurrentUser;
use crate::error::ApiError;
use crate::routes::auth::PublicUser;
use crate::routes::files::{FileView, file_view, serve_blob};
use crate::state::AppState;
use crate::storage::sha256_hex;
use crate::{time, trash};
use entity::{File, Role, User};

/// Like `files::router`: the upload route gets the limit (or none, when
/// building the routes for the OpenAPI document — `routes::api_doc`).
pub fn router(max_upload_bytes: Option<u64>) -> OpenApiRouter<AppState> {
    let upload = OpenApiRouter::new().routes(routes!(upload_file_version));
    let upload = match max_upload_bytes {
        Some(max) => upload.layer(DefaultBodyLimit::max(max as usize + 1024 * 1024)),
        None => upload,
    };
    OpenApiRouter::new()
        .merge(upload)
        .routes(routes!(list_file_versions))
        .routes(routes!(file_version_content))
        .routes(routes!(delete_file))
}

/// Record the first version of a freshly uploaded file.
pub(crate) async fn record_first(db: &impl Db, file: &File) -> Result<(), ApiError> {
    file_versions::ActiveModel {
        id: Set(uuid::Uuid::now_v7()),
        file_id: Set(file.id),
        number: Set(1),
        name: Set(file.name.clone()),
        mime: Set(file.mime.clone()),
        size: Set(file.size),
        sha256: Set(file.sha256.clone()),
        storage_backend: Set(file.storage_backend.clone()),
        storage_key: Set(file.storage_key.clone()),
        uploaded_by: Set(file.uploaded_by),
        created_at: Set(file.created_at),
    }
    .insert(db)
    .await?;
    Ok(())
}

#[derive(Serialize, ToSchema)]
pub struct FileVersionView {
    pub id: uuid::Uuid,
    /// 1 for the first upload.
    pub number: i64,
    pub name: String,
    pub mime: String,
    /// Size in bytes.
    pub size: i64,
    pub uploaded_by: Option<PublicUser>,
    #[serde(with = "time::utc")]
    #[schema(value_type = String, format = DateTime)]
    pub created_at: DateTime,
    /// The version the file shows now.
    pub current: bool,
}

/// A live file the caller may access at `min_role`.
async fn load(db: &impl Db, user: &User, id: uuid::Uuid, min_role: Role) -> Result<File, ApiError> {
    let file = files::Entity::find_by_id(id).one_or_not_found(db).await?;
    if file.deleted_at.is_some() {
        return Err(ApiError::not_found());
    }
    access::require_access(db, user, file.workspace_id, min_role).await?;
    Ok(file)
}

/// The versions of a file, newest first.
#[utoipa::path(
    get,
    path = "/files/{id}/versions",
    tag = "files",
    security(("session_cookie" = [])),
    params(("id" = uuid::Uuid, Path, description = "File id")),
    responses(
        (status = 200, description = "Versions, newest first", body = [FileVersionView]),
        (status = 401, description = "Not logged in"),
        (status = 404, description = "No file with that id, or no access")
    )
)]
pub async fn list_file_versions(
    State(state): State<AppState>,
    CurrentUser { user, .. }: CurrentUser,
    Path(id): Path<uuid::Uuid>,
) -> Result<Json<Vec<FileVersionView>>, ApiError> {
    let db = state.db();
    load(db, &user, id, Role::Read).await?;
    let versions = file_versions::Entity::find()
        .filter(file_versions::Column::FileId.eq(id))
        .order_by_desc(file_versions::Column::Number)
        .all(db)
        .await?;
    let uploaders = versions.load_one(users::Entity, db).await?;
    let newest = versions.first().map(|v| v.number);
    Ok(Json(
        versions
            .into_iter()
            .zip(uploaders)
            .map(|(v, uploader)| FileVersionView {
                current: Some(v.number) == newest,
                uploaded_by: uploader.as_ref().map(PublicUser::from),
                id: v.id,
                number: v.number,
                name: v.name,
                mime: v.mime,
                size: v.size,
                created_at: v.created_at,
            })
            .collect(),
    ))
}

/// The bytes of one version.
#[utoipa::path(
    get,
    path = "/files/{id}/versions/{version_id}/content",
    tag = "files",
    security(("session_cookie" = [])),
    params(
        ("id" = uuid::Uuid, Path, description = "File id"),
        ("version_id" = uuid::Uuid, Path, description = "Version id")
    ),
    responses(
        (status = 200, description = "The version's content", content_type = "application/octet-stream"),
        (status = 401, description = "Not logged in"),
        (status = 404, description = "No such version, or no access")
    )
)]
pub async fn file_version_content(
    State(state): State<AppState>,
    CurrentUser { user, .. }: CurrentUser,
    Path((id, version_id)): Path<(uuid::Uuid, uuid::Uuid)>,
) -> Result<Response, ApiError> {
    let version = {
        let db = state.db();
        load(db, &user, id, Role::Read).await?;
        let version = file_versions::Entity::find_by_id(version_id)
            .one_or_not_found(db)
            .await?;
        if version.file_id != id {
            return Err(ApiError::not_found());
        }
        version
    };
    serve_blob(
        &state,
        &version.storage_backend,
        &version.storage_key,
        &version.mime,
        &version.name,
    )
    .await
}

/// Upload a new version (multipart `file` part). The file keeps its name and id; its content, type and
/// size become the new version's. Identical content to the current version is refused (409).
#[utoipa::path(
    post,
    path = "/files/{id}/versions",
    tag = "files",
    security(("session_cookie" = [])),
    params(("id" = uuid::Uuid, Path, description = "File id")),
    responses(
        (status = 201, description = "Version stored", body = FileView),
        (status = 400, description = "Missing or empty file"),
        (status = 401, description = "Not logged in"),
        (status = 404, description = "No file with that id, or no access"),
        (status = 409, description = "Same content as the current version"),
        (status = 413, description = "Larger than the upload limit")
    )
)]
pub async fn upload_file_version(
    State(state): State<AppState>,
    CurrentUser { user, .. }: CurrentUser,
    Path(id): Path<uuid::Uuid>,
    mut multipart: Multipart,
) -> Result<(StatusCode, Json<FileView>), ApiError> {
    load(state.db(), &user, id, Role::Write).await?;

    let mut upload: Option<(Option<String>, Vec<u8>)> = None;
    while let Some(field) = multipart
        .next_field()
        .await
        .map_err(|e| ApiError::new(e.status(), "invalid upload"))?
    {
        if field.name() == Some("file") {
            let content_type = field.content_type().map(str::to_string);
            let bytes = field
                .bytes()
                .await
                .map_err(|e| ApiError::new(e.status(), "upload failed or too large"))?;
            upload = Some((content_type, bytes.to_vec()));
        }
    }
    let (content_type, bytes) =
        upload.ok_or_else(|| ApiError::bad_request("a file is required"))?;
    if bytes.is_empty() {
        return Err(ApiError::bad_request("the file is empty"));
    }
    if bytes.len() as u64 > state.files.max_upload_bytes {
        return Err(ApiError::payload_too_large(format!(
            "files can be at most {} MB",
            state.files.max_upload_bytes / (1024 * 1024)
        )));
    }
    let sha256 = sha256_hex(&bytes);

    let db = state.writer().await?;
    let db = &*db;
    let file = files::Entity::find_by_id(id).one_or_not_found(db).await?;
    if file.sha256 == sha256 {
        return Err(ApiError::conflict(
            "this is the same content as the current version",
        ));
    }
    let mime = content_type.filter(|c| !c.is_empty()).unwrap_or_else(|| {
        mime_guess::from_path(&file.name)
            .first_or_octet_stream()
            .to_string()
    });
    let store = state.files.default_store();
    let key = format!("{}/{}", file.workspace_id, uuid::Uuid::new_v4());
    store.put(&key, bytes.clone()).await.map_err(|e| {
        tracing::error!(error = %e, "storing an upload failed");
        ApiError::internal("could not store the file")
    })?;

    let workspace_id = file.workspace_id;
    let name = file.name.clone();
    let next = file_versions::Entity::find()
        .filter(file_versions::Column::FileId.eq(id))
        .all(db)
        .await?
        .iter()
        .map(|v| v.number)
        .max()
        .unwrap_or(0)
        + 1;
    let result: Result<(), ApiError> = async {
        let tx = db.begin().await?;
        file_versions::ActiveModel {
            id: Set(uuid::Uuid::now_v7()),
            file_id: Set(id),
            number: Set(next),
            name: Set(name.clone()),
            mime: Set(mime.clone()),
            size: Set(bytes.len() as i64),
            sha256: Set(sha256.clone()),
            storage_backend: Set(store.name().to_string()),
            storage_key: Set(key.clone()),
            uploaded_by: Set(user.id),
            created_at: Set(time::now()),
        }
        .insert(&tx)
        .await?;
        files::ActiveModel {
            id: Unchanged(file.id),
            mime: Set(mime.clone()),
            size: Set(bytes.len() as i64),
            sha256: Set(sha256.clone()),
            storage_backend: Set(store.name().to_string()),
            storage_key: Set(key.clone()),
            ..Default::default()
        }
        .update(&tx)
        .await?;
        audit::record(
            &tx,
            Entry::feed(workspace_id, user.id, "file_version_added")
                .target("file", id)
                .data(serde_json::json!({ "file": name, "version": next })),
        )
        .await?;
        tx.commit().await?;
        Ok(())
    }
    .await;
    if let Err(e) = result {
        let _ = store.delete(&key).await; // do not leave an orphan behind
        return Err(e);
    }
    let file = files::Entity::find_by_id(id).one_or_not_found(db).await?;
    Ok((StatusCode::CREATED, Json(file_view(db, file).await?)))
}

/// Delete a file (it moves to the trash). Its attachments stay recorded and work again if it is restored.
#[utoipa::path(
    delete,
    path = "/files/{id}",
    tag = "files",
    security(("session_cookie" = [])),
    params(("id" = uuid::Uuid, Path, description = "File id")),
    responses(
        (status = 204, description = "File deleted"),
        (status = 401, description = "Not logged in"),
        (status = 404, description = "No file with that id, or no access")
    )
)]
pub async fn delete_file(
    State(state): State<AppState>,
    CurrentUser { user, .. }: CurrentUser,
    Path(id): Path<uuid::Uuid>,
) -> Result<StatusCode, ApiError> {
    let db = state.writer().await?;
    let file = load(&*db, &user, id, Role::Write).await?;
    let tx = db.begin().await?;
    trash::delete_file(&tx, &file, time::now()).await?;
    audit::deleted(&tx, user.id, file.workspace_id, "file", id, &file.name).await?;
    tx.commit().await?;
    Ok(StatusCode::NO_CONTENT)
}
