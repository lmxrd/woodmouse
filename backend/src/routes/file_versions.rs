//! File versions: uploading a new version of a library file keeps the old content, under the same file id
//! (so attachments and page blocks keep working). Deleting a file is soft (Trash).

use axum::Json;
use axum::extract::{DefaultBodyLimit, Multipart, Path, State};
use axum::http::StatusCode;
use axum::response::Response;
use jiff::civil::DateTime;
use serde::Serialize;
use sha2::{Digest, Sha256};
use utoipa::ToSchema;
use utoipa_axum::{router::OpenApiRouter, routes};

use crate::access;
use crate::audit::{self, Entry};
use crate::auth::CurrentUser;
use crate::error::ApiError;
use crate::models::{File, FileVersion, Role, User};
use crate::routes::auth::{People, PublicUser};
use crate::routes::files::{FileView, file_view, serve_blob};
use crate::state::AppState;
use crate::{time, trash};

pub fn router(max_upload_bytes: u64) -> OpenApiRouter<AppState> {
    let upload = OpenApiRouter::new()
        .routes(routes!(upload_file_version))
        .layer(DefaultBodyLimit::max(
            max_upload_bytes as usize + 1024 * 1024,
        ));
    OpenApiRouter::new()
        .merge(upload)
        .routes(routes!(list_file_versions))
        .routes(routes!(file_version_content))
        .routes(routes!(delete_file))
}

/// Record the first version of a freshly uploaded file.
pub(crate) async fn record_first(
    db: &mut dyn toasty::Executor,
    file: &File,
) -> Result<(), ApiError> {
    toasty::create!(FileVersion {
        file_id: file.id,
        number: 1,
        name: file.name.clone(),
        mime: file.mime.clone(),
        size: file.size,
        sha256: file.sha256.clone(),
        storage_backend: file.storage_backend.clone(),
        storage_key: file.storage_key.clone(),
        uploaded_by: file.uploaded_by,
        created_at: file.created_at,
    })
    .exec(db)
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
async fn load(
    db: &mut dyn toasty::Executor,
    user: &User,
    id: uuid::Uuid,
    min_role: Role,
) -> Result<File, ApiError> {
    let file = File::get_by_id(db, &id).await?;
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
    let db = &mut state.db();
    load(db, &user, id, Role::Read).await?;
    let mut versions = FileVersion::filter_by_file_id(id).exec(db).await?;
    versions.sort_by_key(|v| std::cmp::Reverse(v.number));
    let people = People::load(db, versions.iter().map(|v| v.uploaded_by)).await?;
    let newest = versions.first().map(|v| v.number);
    Ok(Json(
        versions
            .into_iter()
            .map(|v| FileVersionView {
                current: Some(v.number) == newest,
                uploaded_by: people.public(v.uploaded_by),
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
        let db = &mut state.db();
        load(db, &user, id, Role::Read).await?;
        let version = FileVersion::get_by_id(db, &version_id).await?;
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
    load(&mut state.db(), &user, id, Role::Write).await?;

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
    let sha256 = format!("{:x}", Sha256::digest(&bytes));

    let mut db = state.writer().await;
    let db = &mut *db;
    let mut file = File::get_by_id(db, &id).await?;
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
    let next = FileVersion::filter_by_file_id(id)
        .exec(db)
        .await?
        .iter()
        .map(|v| v.number)
        .max()
        .unwrap_or(0)
        + 1;
    let result: Result<(), ApiError> = async {
        let mut tx = db.transaction().await?;
        toasty::create!(FileVersion {
            file_id: id,
            number: next,
            name: name.clone(),
            mime: mime.clone(),
            size: bytes.len() as i64,
            sha256: sha256.clone(),
            storage_backend: store.name().to_string(),
            storage_key: key.clone(),
            uploaded_by: user.id,
            created_at: time::now(),
        })
        .exec(&mut tx)
        .await?;
        file.update()
            .mime(mime.clone())
            .size(bytes.len() as i64)
            .sha256(sha256.clone())
            .storage_backend(store.name().to_string())
            .storage_key(key.clone())
            .exec(&mut tx)
            .await?;
        audit::record(
            &mut tx,
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
    let file = File::get_by_id(db, &id).await?;
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
    let mut db = state.writer().await;
    let file = load(&mut *db, &user, id, Role::Write).await?;
    let mut tx = db.transaction().await?;
    trash::delete_file(&mut tx, &file, time::now()).await?;
    audit::deleted(&mut tx, user.id, file.workspace_id, "file", id, &file.name).await?;
    tx.commit().await?;
    Ok(StatusCode::NO_CONTENT)
}
