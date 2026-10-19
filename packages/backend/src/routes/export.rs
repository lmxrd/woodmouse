//! Markdown export of wiki pages: one page, or the whole workspace wiki as a zip.

use std::collections::{HashMap, HashSet};
use std::io::Write;

use axum::extract::{Path, State};
use axum::response::Response;
use utoipa_axum::{router::OpenApiRouter, routes};

use crate::access;
use crate::api::attachment;
use crate::auth::CurrentUser;
use crate::error::ApiError;
use crate::markdown::page_to_markdown;
use crate::models::{Role, WikiPage};
use crate::pages::blocks_of;
use crate::routes::pages::live_pages;
use crate::state::AppState;

pub fn router() -> OpenApiRouter<AppState> {
    OpenApiRouter::new()
        .routes(routes!(export_page))
        .routes(routes!(export_wiki))
}

/// A name that is safe as a file or folder name on any system.
fn file_name(title: &str) -> String {
    let cleaned: String = title
        .chars()
        .map(|c| {
            if c.is_control() || r#"/\:*?"<>|"#.contains(c) {
                '-'
            } else {
                c
            }
        })
        .collect();
    let cleaned = cleaned.trim().trim_matches('.').trim();
    if cleaned.is_empty() {
        "Untitled".to_string()
    } else {
        cleaned.chars().take(100).collect()
    }
}

/// Download a page as a Markdown file.
#[utoipa::path(
    get,
    path = "/pages/{id}/export",
    tag = "wiki",
    security(("session_cookie" = [])),
    params(("id" = uuid::Uuid, Path, description = "Page id")),
    responses(
        (status = 200, description = "The page as Markdown", body = String, content_type = "text/markdown"),
        (status = 404, description = "No such page, or no access")
    )
)]
pub async fn export_page(
    State(state): State<AppState>,
    CurrentUser { user, .. }: CurrentUser,
    Path(id): Path<uuid::Uuid>,
) -> Result<Response, ApiError> {
    let db = &mut state.db();
    let page = access::require_page_access(db, &user, id, Role::Read).await?;
    let blocks = blocks_of(&page)?;
    // Other pages are not part of this file, so links to them keep only their label.
    let markdown = page_to_markdown(&page.title, &blocks, &|_| None);
    Ok(attachment(
        &format!("{}.md", file_name(&page.title)),
        "text/markdown; charset=utf-8",
        markdown.into_bytes(),
    ))
}

/// Where each page goes in the archive: folders follow the tree, a page with subpages gets a folder of
/// the same name next to its file. Names that collide get a number.
fn archive_paths(pages: &[WikiPage]) -> HashMap<uuid::Uuid, Vec<String>> {
    let mut children: HashMap<Option<uuid::Uuid>, Vec<&WikiPage>> = HashMap::new();
    let ids: HashSet<uuid::Uuid> = pages.iter().map(|p| p.id).collect();
    for p in pages {
        let parent = p.parent_id.filter(|id| ids.contains(id));
        children.entry(parent).or_default().push(p);
    }
    for list in children.values_mut() {
        list.sort_by_key(|p| (p.position, p.id));
    }
    fn walk(
        parent: Option<uuid::Uuid>,
        dir: &[String],
        children: &HashMap<Option<uuid::Uuid>, Vec<&WikiPage>>,
        out: &mut HashMap<uuid::Uuid, Vec<String>>,
    ) {
        let mut used: HashSet<String> = HashSet::new();
        for page in children.get(&parent).into_iter().flatten() {
            let base = file_name(&page.title);
            let mut name = base.clone();
            let mut n = 2;
            while !used.insert(name.to_lowercase()) {
                name = format!("{base} ({n})");
                n += 1;
            }
            let mut path = dir.to_vec();
            path.push(format!("{name}.md"));
            out.insert(page.id, path);
            let mut sub = dir.to_vec();
            sub.push(name);
            walk(Some(page.id), &sub, children, out);
        }
    }
    let mut out = HashMap::new();
    walk(None, &[], &children, &mut out);
    out
}

/// The link from the file at `from` to the file at `to` (both paths inside the archive).
fn relative_link(from: &[String], to: &[String]) -> String {
    let from_dir = &from[..from.len() - 1];
    let common = from_dir.iter().zip(to).take_while(|(a, b)| a == b).count();
    let mut parts: Vec<String> = vec!["..".to_string(); from_dir.len() - common];
    parts.extend(to[common..].iter().map(|p| crate::api::percent_encode(p)));
    parts.join("/")
}

/// Download the whole wiki as a zip of Markdown files, folders following the page tree.
#[utoipa::path(
    get,
    path = "/workspaces/{id}/wiki/export",
    tag = "wiki",
    security(("session_cookie" = [])),
    params(("id" = uuid::Uuid, Path, description = "Workspace id")),
    responses(
        (status = 200, description = "A zip archive of Markdown files", body = Vec<u8>, content_type = "application/zip"),
        (status = 404, description = "No such workspace, or not a member")
    )
)]
pub async fn export_wiki(
    State(state): State<AppState>,
    CurrentUser { user, .. }: CurrentUser,
    Path(workspace_id): Path<uuid::Uuid>,
) -> Result<Response, ApiError> {
    let db = &mut state.db();
    access::require_access(db, &user, workspace_id, Role::Read).await?;
    let pages = live_pages(db, workspace_id).await?;
    let paths = archive_paths(&pages);

    let mut zip = zip::ZipWriter::new(std::io::Cursor::new(Vec::new()));
    let options = zip::write::SimpleFileOptions::default()
        .compression_method(zip::CompressionMethod::Deflated);
    let zip_err = |e: &dyn std::fmt::Display| {
        tracing::error!(%e, "building the export archive");
        ApiError::internal("could not build the archive")
    };
    for page in &pages {
        let Some(path) = paths.get(&page.id) else {
            continue;
        };
        let blocks = blocks_of(page)?;
        let href = |target: uuid::Uuid| paths.get(&target).map(|t| relative_link(path, t));
        let markdown = page_to_markdown(&page.title, &blocks, &href);
        zip.start_file(path.join("/"), options)
            .map_err(|e| zip_err(&e))?;
        zip.write_all(markdown.as_bytes())
            .map_err(|e| zip_err(&e))?;
    }
    let bytes = zip.finish().map_err(|e| zip_err(&e))?.into_inner();
    Ok(attachment("wiki.zip", "application/zip", bytes))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn file_names_are_safe() {
        assert_eq!(file_name("a/b: c?"), "a-b- c-");
        assert_eq!(file_name("  .. "), "Untitled");
    }

    #[test]
    fn links_between_files_are_relative() {
        let a = vec!["Home".to_string(), "Plan.md".to_string()];
        let b = vec![
            "Home".to_string(),
            "Plan".to_string(),
            "Sub page.md".to_string(),
        ];
        assert_eq!(relative_link(&a, &b), "Plan/Sub%20page.md");
        assert_eq!(relative_link(&b, &a), "../Plan.md");
        let top = vec!["Other.md".to_string()];
        assert_eq!(relative_link(&b, &top), "../../Other.md");
    }
}
