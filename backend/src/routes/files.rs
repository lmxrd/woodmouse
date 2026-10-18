//! The workspace file library and task attachments.
//!
//! Uploads are deduplicated per workspace by SHA-256. A file stays in the library when it is detached
//! from a task or the task is deleted; only an admin's cleanup removes files nothing references.

use axum::Json;
use axum::extract::{DefaultBodyLimit, Multipart, Path, Query, State};
use axum::http::{HeaderMap, HeaderValue, StatusCode, header};
use axum::response::{IntoResponse, Response};
use jiff::civil::DateTime;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use toasty::stmt::in_list;
use utoipa::{IntoParams, ToSchema};
use utoipa_axum::{router::OpenApiRouter, routes};

use crate::access;
use crate::audit::{self, Entry};
use crate::auth::CurrentUser;
use crate::error::ApiError;
use crate::models::{File, FileRef, FileVersion, Role, Task, User, WikiPage};
use crate::routes::auth::{People, PublicUser};
use crate::state::AppState;
use crate::time;

/// Bodies of other routes stay small; uploads get the configured limit plus a little for the
/// multipart framing. `None` builds the routes for the OpenAPI document alone
/// (`routes::api_doc`): the limit is a layer, not an operation.
pub fn router(max_upload_bytes: Option<u64>) -> OpenApiRouter<AppState> {
    let upload = OpenApiRouter::new().routes(routes!(upload_file));
    let upload = match max_upload_bytes {
        Some(max) => upload.layer(DefaultBodyLimit::max(max as usize + 1024 * 1024)),
        None => upload,
    };
    OpenApiRouter::new()
        .merge(upload)
        .routes(routes!(list_files))
        .routes(routes!(file_content))
        .routes(routes!(file_usage))
        .routes(routes!(list_attachments, attach_file))
        .routes(routes!(detach_file))
        .routes(routes!(list_page_attachments, attach_page_file))
        .routes(routes!(detach_page_file))
        .routes(routes!(unused_files))
        .routes(routes!(cleanup_files))
        .routes(routes!(get_config))
}

/// Raster image types that are safe to display inline. Everything else (including SVG and HTML)
/// is only ever served as a download.
const INLINE_TYPES: [&str; 4] = ["image/png", "image/jpeg", "image/gif", "image/webp"];

#[derive(Serialize, ToSchema, Clone)]
pub struct FileView {
    /// How many versions the file has (1 until a new version is uploaded).
    pub versions: usize,
    pub id: uuid::Uuid,
    pub workspace_id: uuid::Uuid,
    pub name: String,
    pub mime: String,
    /// Size in bytes.
    pub size: i64,
    pub sha256: String,
    /// True for image types that can be shown inline (`/files/{id}/content`).
    pub previewable: bool,
    pub uploaded_by: Option<PublicUser>,
    #[serde(with = "time::utc")]
    #[schema(value_type = String, format = DateTime)]
    pub created_at: DateTime,
    /// How many tasks the file is attached to (including trashed ones, which can come back).
    pub used_in: usize,
}

#[derive(Serialize, ToSchema)]
pub struct DuplicateFile {
    pub error: String,
    /// The file already in the library with identical content.
    pub existing_file: FileView,
}

#[derive(Serialize, ToSchema)]
pub struct AppConfig {
    /// Largest accepted upload, in bytes.
    pub max_upload_bytes: u64,
}

#[derive(Deserialize, IntoParams)]
#[into_params(parameter_in = Query)]
pub struct FileFilters {
    /// Only files whose name contains this text (case-insensitive).
    pub q: Option<String>,
    /// Only files attached to nothing.
    pub unused: Option<bool>,
}

async fn views(db: &mut dyn toasty::Executor, files: Vec<File>) -> Result<Vec<FileView>, ApiError> {
    if files.is_empty() {
        return Ok(vec![]);
    }
    let file_ids: Vec<uuid::Uuid> = files.iter().map(|f| f.id).collect();
    let refs = FileRef::all()
        .filter(in_list(FileRef::fields().file_id(), file_ids))
        .exec(db)
        .await?;
    let version_rows = FileVersion::all()
        .filter(in_list(
            FileVersion::fields().file_id(),
            files.iter().map(|f| f.id).collect::<Vec<_>>(),
        ))
        .exec(db)
        .await?;
    let people = People::load(db, files.iter().map(|f| f.uploaded_by)).await?;
    Ok(files
        .into_iter()
        .map(|f| FileView {
            id: f.id,
            workspace_id: f.workspace_id,
            previewable: INLINE_TYPES.contains(&f.mime.as_str()),
            uploaded_by: people.public(f.uploaded_by),
            used_in: refs.iter().filter(|r| r.file_id == f.id).count(),
            versions: version_rows
                .iter()
                .filter(|v| v.file_id == f.id)
                .count()
                .max(1),
            name: f.name,
            mime: f.mime,
            size: f.size,
            sha256: f.sha256,
            created_at: f.created_at,
        })
        .collect())
}

/// One file as the API shows it.
pub(crate) async fn file_view(
    db: &mut dyn toasty::Executor,
    file: File,
) -> Result<FileView, ApiError> {
    one_view(db, file).await
}

async fn one_view(db: &mut dyn toasty::Executor, file: File) -> Result<FileView, ApiError> {
    Ok(views(db, vec![file]).await?.swap_remove(0))
}

/// A file the caller may read; returns it with its workspace.
async fn load_file(
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

fn clean_name(raw: &str) -> String {
    let base = std::path::Path::new(raw)
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or("file");
    let name: String = base.chars().filter(|c| !c.is_control()).take(200).collect();
    if name.trim().is_empty() {
        "file".to_string()
    } else {
        name
    }
}

/// The workspace's files, newest first.
#[utoipa::path(
    get,
    path = "/workspaces/{id}/files",
    tag = "files",
    security(("session_cookie" = [])),
    params(("id" = uuid::Uuid, Path, description = "Workspace id"), FileFilters),
    responses(
        (status = 200, description = "The workspace's files", body = [FileView]),
        (status = 401, description = "Not logged in"),
        (status = 404, description = "No workspace with that id, or no access")
    )
)]
pub async fn list_files(
    State(state): State<AppState>,
    CurrentUser { user, .. }: CurrentUser,
    Path(id): Path<uuid::Uuid>,
    Query(filters): Query<FileFilters>,
) -> Result<Json<Vec<FileView>>, ApiError> {
    let db = &mut state.db();
    access::require_access(db, &user, id, Role::Read).await?;
    let mut files = File::filter_by_workspace_id(id).exec(db).await?;
    files.retain(|f| f.deleted_at.is_none());
    if let Some(q) = filters
        .q
        .as_ref()
        .map(|q| q.to_lowercase())
        .filter(|q| !q.is_empty())
    {
        files.retain(|f| f.name.to_lowercase().contains(&q));
    }
    files.sort_by_key(|f| std::cmp::Reverse(f.created_at));
    let mut out = views(db, files).await?;
    if filters.unused == Some(true) {
        out.retain(|f| f.used_in == 0);
    }
    Ok(Json(out))
}

/// Upload a file (multipart: a `file` part, and optionally a `task_id` text part to attach it).
/// Content already in the library is not stored again: the response is 409 with the existing file.
#[utoipa::path(
    post,
    path = "/workspaces/{id}/files",
    tag = "files",
    security(("session_cookie" = [])),
    params(("id" = uuid::Uuid, Path, description = "Workspace id")),
    responses(
        (status = 201, description = "File stored", body = FileView),
        (status = 400, description = "Missing or empty file"),
        (status = 401, description = "Not logged in"),
        (status = 404, description = "No workspace with that id, or no access"),
        (status = 409, description = "Identical content is already in the library", body = DuplicateFile),
        (status = 413, description = "Larger than the upload limit")
    )
)]
pub async fn upload_file(
    State(state): State<AppState>,
    CurrentUser { user, .. }: CurrentUser,
    Path(id): Path<uuid::Uuid>,
    mut multipart: Multipart,
) -> Result<Response, ApiError> {
    access::require_access(&mut state.db(), &user, id, Role::Write).await?;

    let mut upload: Option<(String, Option<String>, Vec<u8>)> = None;
    let mut task_id: Option<uuid::Uuid> = None;
    while let Some(field) = multipart
        .next_field()
        .await
        .map_err(|e| ApiError::new(e.status(), "invalid upload"))?
    {
        match field.name() {
            Some("file") => {
                let name = clean_name(field.file_name().unwrap_or("file"));
                let content_type = field.content_type().map(str::to_string);
                let bytes = field
                    .bytes()
                    .await
                    .map_err(|e| ApiError::new(e.status(), "upload failed or too large"))?;
                upload = Some((name, content_type, bytes.to_vec()));
            }
            Some("task_id") => {
                let text = field
                    .text()
                    .await
                    .map_err(|_| ApiError::bad_request("invalid task_id"))?;
                task_id = Some(
                    text.trim()
                        .parse()
                        .map_err(|_| ApiError::bad_request("invalid task_id"))?,
                );
            }
            _ => {}
        }
    }
    let (name, content_type, bytes) =
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
    let mime = content_type.filter(|c| !c.is_empty()).unwrap_or_else(|| {
        mime_guess::from_path(&name)
            .first_or_octet_stream()
            .to_string()
    });

    let mut db = state.writer().await;
    let db = &mut *db;
    if let Some(task_id) = task_id {
        let workspace = access::require_task_access(db, &user, task_id, Role::Write).await?;
        if workspace != id {
            return Err(ApiError::not_found());
        }
    }

    // Identical content already in this workspace: do not store it twice.
    if let Some(existing) = File::filter_by_sha256(sha256.clone())
        .exec(db)
        .await?
        .into_iter()
        .find(|f| f.workspace_id == id && f.deleted_at.is_none())
    {
        let body = DuplicateFile {
            error: "this file is already in the library".to_string(),
            existing_file: one_view(db, existing).await?,
        };
        return Ok((StatusCode::CONFLICT, Json(body)).into_response());
    }

    let store = state.files.default_store();
    let key = format!("{id}/{}", uuid::Uuid::new_v4());
    store.put(&key, bytes.clone()).await.map_err(|e| {
        tracing::error!(error = %e, "storing an upload failed");
        ApiError::internal("could not store the file")
    })?;
    let created = toasty::create!(File {
        workspace_id: id,
        name,
        mime,
        size: bytes.len() as i64,
        sha256,
        storage_backend: store.name().to_string(),
        storage_key: key.clone(),
        uploaded_by: user.id,
        created_at: time::now(),
    })
    .exec(db)
    .await;
    let file = match created {
        Ok(file) => file,
        Err(e) => {
            let _ = store.delete(&key).await; // best effort: do not leave an orphan behind
            return Err(e.into());
        }
    };
    let mut tx = db.transaction().await?;
    crate::routes::file_versions::record_first(&mut tx, &file).await?;
    if let Some(task_id) = task_id {
        attach(&mut tx, id, task_id, user.id, &file).await?;
    }
    tx.commit().await?;
    let view = one_view(db, file).await?;
    Ok((StatusCode::CREATED, Json(view)).into_response())
}

/// The file's bytes. Raster images are shown inline; everything else downloads.
#[utoipa::path(
    get,
    path = "/files/{id}/content",
    tag = "files",
    security(("session_cookie" = [])),
    params(("id" = uuid::Uuid, Path, description = "File id")),
    responses(
        (status = 200, description = "The file's content", content_type = "application/octet-stream"),
        (status = 401, description = "Not logged in"),
        (status = 404, description = "No file with that id, or no access")
    )
)]
pub async fn file_content(
    State(state): State<AppState>,
    CurrentUser { user, .. }: CurrentUser,
    Path(id): Path<uuid::Uuid>,
) -> Result<Response, ApiError> {
    let file = { load_file(&mut state.db(), &user, id, Role::Read).await? };
    serve_blob(
        &state,
        &file.storage_backend,
        &file.storage_key,
        &file.mime,
        &file.name,
    )
    .await
}

/// Serve stored bytes: raster images inline, everything else as a download, with headers that stop a
/// browser from treating it as a page.
pub(crate) async fn serve_blob(
    state: &AppState,
    backend: &str,
    key: &str,
    mime: &str,
    name: &str,
) -> Result<Response, ApiError> {
    let store = state
        .files
        .store(backend)
        .ok_or_else(|| ApiError::internal("this file's storage is not configured"))?;
    let bytes = store.get(key).await.map_err(|e| {
        tracing::error!(error = %e, key, "reading a stored file failed");
        ApiError::not_found()
    })?;

    let inline = INLINE_TYPES.contains(&mime);
    let mut headers = HeaderMap::new();
    let content_type = if inline {
        mime
    } else {
        "application/octet-stream"
    };
    headers.insert(
        header::CONTENT_TYPE,
        HeaderValue::from_str(content_type).expect("static types"),
    );
    // RFC 5987 encoding keeps any file name header-safe.
    let encoded: String = name
        .bytes()
        .map(|b| {
            if b.is_ascii_alphanumeric() || b"-._~".contains(&b) {
                (b as char).to_string()
            } else {
                format!("%{b:02X}")
            }
        })
        .collect();
    let disposition = format!(
        "{}; filename*=UTF-8''{encoded}",
        if inline { "inline" } else { "attachment" }
    );
    headers.insert(
        header::CONTENT_DISPOSITION,
        HeaderValue::from_str(&disposition).map_err(|_| ApiError::internal("bad file name"))?,
    );
    headers.insert(
        header::X_CONTENT_TYPE_OPTIONS,
        HeaderValue::from_static("nosniff"),
    );
    headers.insert(
        header::CONTENT_SECURITY_POLICY,
        HeaderValue::from_static("default-src 'none'; sandbox"),
    );
    headers.insert(
        header::CACHE_CONTROL,
        HeaderValue::from_static("private, max-age=300"),
    );
    Ok((headers, bytes).into_response())
}

#[derive(Serialize, ToSchema)]
pub struct FileUsage {
    /// `task` or `page`.
    pub kind: String,
    /// The task's or page's id.
    pub id: uuid::Uuid,
    /// Tasks only: the project it is in.
    pub project_id: Option<uuid::Uuid>,
    pub title: String,
    /// True when it is in the trash.
    pub in_trash: bool,
}

/// Where a file is attached.
#[utoipa::path(
    get,
    path = "/files/{id}/usage",
    tag = "files",
    security(("session_cookie" = [])),
    params(("id" = uuid::Uuid, Path, description = "File id")),
    responses(
        (status = 200, description = "The tasks the file is attached to", body = [FileUsage]),
        (status = 401, description = "Not logged in"),
        (status = 404, description = "No file with that id, or no access")
    )
)]
pub async fn file_usage(
    State(state): State<AppState>,
    CurrentUser { user, .. }: CurrentUser,
    Path(id): Path<uuid::Uuid>,
) -> Result<Json<Vec<FileUsage>>, ApiError> {
    let db = &mut state.db();
    load_file(db, &user, id, Role::Read).await?;
    let refs = FileRef::filter_by_file_id(id).exec(db).await?;
    let ids_of = |kind: &str| -> Vec<uuid::Uuid> {
        refs.iter()
            .filter(|r| r.target_type == kind)
            .map(|r| r.target_id)
            .collect()
    };
    let mut out = vec![];
    if !ids_of(TASK).is_empty() {
        for t in Task::all()
            .filter(in_list(Task::fields().id(), ids_of(TASK)))
            .exec(db)
            .await?
        {
            out.push(FileUsage {
                kind: TASK.to_string(),
                id: t.id,
                project_id: Some(t.project_id),
                in_trash: t.deleted_at.is_some(),
                title: t.title,
            });
        }
    }
    if !ids_of(PAGE).is_empty() {
        for p in WikiPage::all()
            .filter(in_list(WikiPage::fields().id(), ids_of(PAGE)))
            .exec(db)
            .await?
        {
            out.push(FileUsage {
                kind: PAGE.to_string(),
                id: p.id,
                project_id: None,
                in_trash: p.deleted_at.is_some(),
                title: p.title,
            });
        }
    }
    Ok(Json(out))
}

/// `FileRef::target_type` of a task attachment.
const TASK: &str = "task";
/// `FileRef::target_type` of a wiki page attachment.
const PAGE: &str = crate::pages::PAGE;

/// Attach `file` to a task and record it in the feed. Call with the transaction of the change.
async fn attach(
    db: &mut dyn toasty::Executor,
    workspace_id: uuid::Uuid,
    task_id: uuid::Uuid,
    actor: uuid::Uuid,
    file: &File,
) -> Result<(), ApiError> {
    let task = Task::get_by_id(db, &task_id).await?;
    toasty::create!(FileRef {
        file_id: file.id,
        target_type: TASK.to_string(),
        target_id: task_id,
        embedded: false,
    })
    .exec(db)
    .await?;
    audit::record(
        db,
        Entry::feed(workspace_id, actor, "file_attached")
            .target("task", task_id)
            .data(serde_json::json!({
                "project_id": task.project_id,
                "task_id": task_id,
                "title": task.title,
                "file": file.name,
            })),
    )
    .await
}

/// The files attached to a task.
#[utoipa::path(
    get,
    path = "/tasks/{id}/attachments",
    tag = "files",
    security(("session_cookie" = [])),
    params(("id" = uuid::Uuid, Path, description = "Task id")),
    responses(
        (status = 200, description = "The task's attachments", body = [FileView]),
        (status = 401, description = "Not logged in"),
        (status = 404, description = "No task with that id, or no access")
    )
)]
pub async fn list_attachments(
    State(state): State<AppState>,
    CurrentUser { user, .. }: CurrentUser,
    Path(id): Path<uuid::Uuid>,
) -> Result<Json<Vec<FileView>>, ApiError> {
    let db = &mut state.db();
    access::require_task_access(db, &user, id, Role::Read).await?;
    let refs: Vec<FileRef> = FileRef::filter_by_target_id(id)
        .exec(db)
        .await?
        .into_iter()
        .filter(|r| r.target_type == TASK)
        .collect();
    if refs.is_empty() {
        return Ok(Json(vec![]));
    }
    let mut files = File::all()
        .filter(in_list(
            File::fields().id(),
            refs.iter().map(|r| r.file_id).collect::<Vec<_>>(),
        ))
        .exec(db)
        .await?;
    files.retain(|f| f.deleted_at.is_none());
    files.sort_by_key(|f| f.created_at);
    Ok(Json(views(db, files).await?))
}

#[derive(Deserialize, ToSchema)]
pub struct AttachFile {
    pub file_id: uuid::Uuid,
}

/// Attach a file from the library to a task. Attaching it twice is harmless.
#[utoipa::path(
    post,
    path = "/tasks/{id}/attachments",
    tag = "files",
    security(("session_cookie" = [])),
    params(("id" = uuid::Uuid, Path, description = "Task id")),
    request_body = AttachFile,
    responses(
        (status = 200, description = "Attached", body = FileView),
        (status = 401, description = "Not logged in"),
        (status = 404, description = "No such task or file, or no access")
    )
)]
pub async fn attach_file(
    State(state): State<AppState>,
    CurrentUser { user, .. }: CurrentUser,
    Path(id): Path<uuid::Uuid>,
    Json(input): Json<AttachFile>,
) -> Result<Json<FileView>, ApiError> {
    let mut db = state.writer().await;
    let db = &mut *db;
    let workspace_id = access::require_task_access(db, &user, id, Role::Write).await?;
    let file = File::get_by_id(db, &input.file_id).await?;
    if file.workspace_id != workspace_id {
        return Err(ApiError::not_found());
    }
    let already = FileRef::filter_by_file_id(file.id)
        .exec(db)
        .await?
        .iter()
        .any(|r| r.target_type == TASK && r.target_id == id);
    if !already {
        let mut tx = db.transaction().await?;
        attach(&mut tx, workspace_id, id, user.id, &file).await?;
        tx.commit().await?;
    }
    Ok(Json(one_view(db, file).await?))
}

/// Detach a file from a task. The file stays in the library.
#[utoipa::path(
    delete,
    path = "/tasks/{id}/attachments/{file_id}",
    tag = "files",
    security(("session_cookie" = [])),
    params(
        ("id" = uuid::Uuid, Path, description = "Task id"),
        ("file_id" = uuid::Uuid, Path, description = "File id")
    ),
    responses(
        (status = 204, description = "Detached"),
        (status = 401, description = "Not logged in"),
        (status = 404, description = "No such attachment, or no access")
    )
)]
pub async fn detach_file(
    State(state): State<AppState>,
    CurrentUser { user, .. }: CurrentUser,
    Path((id, file_id)): Path<(uuid::Uuid, uuid::Uuid)>,
) -> Result<StatusCode, ApiError> {
    let mut db = state.writer().await;
    let db = &mut *db;
    access::require_task_access(db, &user, id, Role::Write).await?;
    let reference = FileRef::filter_by_file_id(file_id)
        .exec(db)
        .await?
        .into_iter()
        .find(|r| r.target_type == TASK && r.target_id == id)
        .ok_or_else(ApiError::not_found)?;
    reference.delete().exec(db).await?;
    Ok(StatusCode::NO_CONTENT)
}

#[derive(Serialize, ToSchema)]
pub struct UnusedFiles {
    pub count: usize,
    /// Total size in bytes.
    pub total_bytes: i64,
}

async fn find_unused(
    db: &mut dyn toasty::Executor,
    workspace_id: uuid::Uuid,
) -> Result<Vec<File>, ApiError> {
    let files: Vec<File> = File::filter_by_workspace_id(workspace_id)
        .exec(db)
        .await?
        .into_iter()
        .filter(|f| f.deleted_at.is_none())
        .collect();
    if files.is_empty() {
        return Ok(vec![]);
    }
    let used: std::collections::HashSet<uuid::Uuid> = FileRef::all()
        .filter(in_list(
            FileRef::fields().file_id(),
            files.iter().map(|f| f.id).collect::<Vec<_>>(),
        ))
        .exec(db)
        .await?
        .into_iter()
        .map(|r| r.file_id)
        .collect();
    Ok(files
        .into_iter()
        .filter(|f| !used.contains(&f.id))
        .collect())
}

/// How many files, and how much space, nothing references. Admins only.
#[utoipa::path(
    get,
    path = "/workspaces/{id}/files/unused",
    tag = "files",
    security(("session_cookie" = [])),
    params(("id" = uuid::Uuid, Path, description = "Workspace id")),
    responses(
        (status = 200, description = "Unreferenced files", body = UnusedFiles),
        (status = 401, description = "Not logged in"),
        (status = 404, description = "No workspace with that id, or not an admin")
    )
)]
pub async fn unused_files(
    State(state): State<AppState>,
    CurrentUser { user, .. }: CurrentUser,
    Path(id): Path<uuid::Uuid>,
) -> Result<Json<UnusedFiles>, ApiError> {
    let db = &mut state.db();
    access::require_access(db, &user, id, Role::Admin).await?;
    let unused = find_unused(db, id).await?;
    Ok(Json(UnusedFiles {
        count: unused.len(),
        total_bytes: unused.iter().map(|f| f.size).sum(),
    }))
}

#[derive(Serialize, ToSchema)]
pub struct CleanupResult {
    pub deleted: usize,
    pub freed_bytes: i64,
}

/// Permanently delete every file nothing references (including from storage). Admins only. A file
/// attached to a task in the trash still counts as referenced.
#[utoipa::path(
    post,
    path = "/workspaces/{id}/files/cleanup",
    tag = "files",
    security(("session_cookie" = [])),
    params(("id" = uuid::Uuid, Path, description = "Workspace id")),
    responses(
        (status = 200, description = "What was removed", body = CleanupResult),
        (status = 401, description = "Not logged in"),
        (status = 404, description = "No workspace with that id, or not an admin")
    )
)]
pub async fn cleanup_files(
    State(state): State<AppState>,
    CurrentUser { user, .. }: CurrentUser,
    Path(id): Path<uuid::Uuid>,
) -> Result<Json<CleanupResult>, ApiError> {
    let mut db = state.writer().await;
    let db = &mut *db;
    access::require_access(db, &user, id, Role::Admin).await?;
    let mut result = CleanupResult {
        deleted: 0,
        freed_bytes: 0,
    };
    for file in find_unused(db, id).await? {
        let versions = FileVersion::filter_by_file_id(file.id).exec(db).await?;
        // Every version's bytes, and the current ones even if no version row names them.
        let mut blobs: Vec<(String, String, i64)> = versions
            .iter()
            .map(|v| (v.storage_backend.clone(), v.storage_key.clone(), v.size))
            .collect();
        if !blobs.iter().any(|(_, k, _)| *k == file.storage_key) {
            blobs.push((
                file.storage_backend.clone(),
                file.storage_key.clone(),
                file.size,
            ));
        }
        for (backend, key, size) in blobs {
            if let Some(store) = state.files.store(&backend) {
                store.delete(&key).await.map_err(|e| {
                    tracing::error!(error = %e, file = %file.id, "deleting a stored file failed");
                    ApiError::internal("could not delete a stored file")
                })?;
            }
            result.freed_bytes += size;
        }
        for v in versions {
            v.delete().exec(db).await?;
        }
        result.deleted += 1;
        file.delete().exec(db).await?;
    }
    Ok(Json(result))
}

/// Server settings the client needs.
#[utoipa::path(
    get,
    path = "/config",
    tag = "files",
    security(("session_cookie" = [])),
    responses(
        (status = 200, description = "Server configuration", body = AppConfig),
        (status = 401, description = "Not logged in")
    )
)]
pub async fn get_config(
    State(state): State<AppState>,
    CurrentUser { .. }: CurrentUser,
) -> Json<AppConfig> {
    Json(AppConfig {
        max_upload_bytes: state.files.max_upload_bytes,
    })
}

/// The files attached to a wiki page.
#[utoipa::path(
    get,
    path = "/pages/{id}/attachments",
    tag = "files",
    security(("session_cookie" = [])),
    params(("id" = uuid::Uuid, Path, description = "Page id")),
    responses(
        (status = 200, description = "The page's attachments", body = [FileView]),
        (status = 401, description = "Not logged in"),
        (status = 404, description = "No page with that id, or no access")
    )
)]
pub async fn list_page_attachments(
    State(state): State<AppState>,
    CurrentUser { user, .. }: CurrentUser,
    Path(id): Path<uuid::Uuid>,
) -> Result<Json<Vec<FileView>>, ApiError> {
    let db = &mut state.db();
    access::require_page_access(db, &user, id, Role::Read).await?;
    let ids: Vec<uuid::Uuid> = FileRef::filter_by_target_id(id)
        .exec(db)
        .await?
        .into_iter()
        .filter(|r| r.target_type == PAGE)
        .map(|r| r.file_id)
        .collect();
    if ids.is_empty() {
        return Ok(Json(vec![]));
    }
    let mut files = File::all()
        .filter(in_list(File::fields().id(), ids))
        .exec(db)
        .await?;
    files.retain(|f| f.deleted_at.is_none());
    files.sort_by_key(|f| f.created_at);
    Ok(Json(views(db, files).await?))
}

/// Attach a file from the library to a wiki page. Attaching it twice is harmless.
#[utoipa::path(
    post,
    path = "/pages/{id}/attachments",
    tag = "files",
    security(("session_cookie" = [])),
    params(("id" = uuid::Uuid, Path, description = "Page id")),
    request_body = AttachFile,
    responses(
        (status = 200, description = "Attached", body = FileView),
        (status = 401, description = "Not logged in"),
        (status = 404, description = "No such page or file, or no access")
    )
)]
pub async fn attach_page_file(
    State(state): State<AppState>,
    CurrentUser { user, .. }: CurrentUser,
    Path(id): Path<uuid::Uuid>,
    Json(input): Json<AttachFile>,
) -> Result<Json<FileView>, ApiError> {
    let mut db = state.writer().await;
    let db = &mut *db;
    let page = access::require_page_access(db, &user, id, Role::Write).await?;
    let file = File::get_by_id(db, &input.file_id).await?;
    if file.workspace_id != page.workspace_id {
        return Err(ApiError::not_found());
    }
    let mut tx = db.transaction().await?;
    crate::pages::attach_file(
        &mut tx,
        id,
        page.workspace_id,
        &page.title,
        &file,
        user.id,
        false,
    )
    .await?;
    tx.commit().await?;
    Ok(Json(one_view(db, file).await?))
}

/// Detach a file from a wiki page. The file stays in the library.
#[utoipa::path(
    delete,
    path = "/pages/{id}/attachments/{file_id}",
    tag = "files",
    security(("session_cookie" = [])),
    params(
        ("id" = uuid::Uuid, Path, description = "Page id"),
        ("file_id" = uuid::Uuid, Path, description = "File id")
    ),
    responses(
        (status = 204, description = "Detached"),
        (status = 401, description = "Not logged in"),
        (status = 404, description = "No such attachment, or no access")
    )
)]
pub async fn detach_page_file(
    State(state): State<AppState>,
    CurrentUser { user, .. }: CurrentUser,
    Path((id, file_id)): Path<(uuid::Uuid, uuid::Uuid)>,
) -> Result<StatusCode, ApiError> {
    let mut db = state.writer().await;
    let db = &mut *db;
    let page = access::require_page_access(db, &user, id, Role::Write).await?;
    let mut tx = db.transaction().await?;
    let detached = crate::pages::detach_file(&mut tx, &page, file_id, user.id).await?;
    tx.commit().await?;
    if !detached {
        return Err(ApiError::not_found());
    }
    Ok(StatusCode::NO_CONTENT)
}
