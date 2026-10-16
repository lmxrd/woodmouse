//! The wiki's own document format ("Model A"): a page is a flat, ordered list of blocks, and a block's
//! text is a list of spans. Nothing here knows about any editor library: the editor converts to and from
//! this format, so storage, search, link extraction, history and export never depend on it.
//!
//! ```json
//! { "type": "heading", "attrs": { "level": 2 },
//!   "spans": [ { "text": "Contractors", "marks": ["bold"] },
//!              { "mention": { "task_id": "...", "label": "Order oak worktop" } },
//!              { "page_link": { "page_id": "...", "label": "Budget" } } ] }
//! ```
//! Lists are flat too: consecutive list-item blocks with an `indent` attribute form a nested list.

use serde::{Deserialize, Serialize};
use utoipa::ToSchema;

use crate::error::ApiError;

pub const MAX_BLOCKS: usize = 5000;
const MAX_SPANS_PER_BLOCK: usize = 2000;
const MAX_TEXT_PER_BLOCK: usize = 50_000;
const MAX_INDENT: u8 = 8;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "snake_case")]
pub enum BlockKind {
    Paragraph,
    /// `attrs.level` 1 to 3.
    Heading,
    /// `attrs.indent` nests it.
    BulletItem,
    NumberedItem,
    /// `attrs.checked` and `attrs.indent`.
    TodoItem,
    /// A single text span; `attrs.language` optional.
    Code,
    Quote,
    /// `attrs.tone`: `info`, `warning`, `success` or `danger`.
    Callout,
    /// No spans.
    Divider,
}

impl BlockKind {
    /// The name stored in the database.
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Paragraph => "paragraph",
            Self::Heading => "heading",
            Self::BulletItem => "bullet_item",
            Self::NumberedItem => "numbered_item",
            Self::TodoItem => "todo_item",
            Self::Code => "code",
            Self::Quote => "quote",
            Self::Callout => "callout",
            Self::Divider => "divider",
        }
    }

    pub fn parse(name: &str) -> Option<Self> {
        serde_json::from_value(serde_json::Value::String(name.to_string())).ok()
    }

    pub fn is_list_item(self) -> bool {
        matches!(self, Self::BulletItem | Self::NumberedItem | Self::TodoItem)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "snake_case")]
pub enum Mark {
    Bold,
    Italic,
    Strike,
    Code,
}

/// Block settings. Which ones apply depends on the block's type; anything else is rejected.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize, ToSchema)]
#[serde(deny_unknown_fields)]
pub struct Attrs {
    pub level: Option<u8>,
    pub indent: Option<u8>,
    pub checked: Option<bool>,
    pub language: Option<String>,
    pub tone: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, ToSchema)]
pub struct TaskMention {
    pub task_id: uuid::Uuid,
    /// The task's title when it was mentioned, shown if the task is gone.
    pub label: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, ToSchema)]
pub struct PageLink {
    pub page_id: uuid::Uuid,
    /// The page's title when it was linked.
    pub label: String,
}

/// A run of inline content: styled text, a task mention, or a page link (exactly one of the three).
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize, ToSchema)]
#[serde(deny_unknown_fields)]
pub struct Span {
    pub text: Option<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub marks: Vec<Mark>,
    /// A link target for `text`: any URL format (https, mailto, tel, app schemes, relative paths) except
    /// `javascript:`, `data:` and `vbscript:`.
    pub link: Option<String>,
    pub mention: Option<TaskMention>,
    pub page_link: Option<PageLink>,
}

/// One block as sent by the editor and returned by the API.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, ToSchema)]
#[serde(deny_unknown_fields)]
pub struct BlockData {
    #[serde(rename = "type")]
    pub kind: BlockKind,
    #[serde(default)]
    pub attrs: Attrs,
    #[serde(default)]
    pub spans: Vec<Span>,
}

fn bad(message: &str) -> ApiError {
    ApiError::bad_request(message.to_string())
}

/// Links may use any URL format (https, mailto, tel, obsidian://, relative paths, ...) except the
/// schemes that run code in the page.
fn safe_link(href: &str) -> bool {
    if href.is_empty() || href.len() > 2000 || href.chars().any(char::is_control) {
        return false;
    }
    // Browsers ignore whitespace inside a scheme, so compare with it removed.
    let compact: String = href
        .chars()
        .filter(|c| !c.is_whitespace())
        .collect::<String>()
        .to_ascii_lowercase();
    !["javascript:", "data:", "vbscript:"]
        .iter()
        .any(|scheme| compact.starts_with(scheme))
}

impl BlockData {
    /// Check the block against its type's rules.
    pub fn validate(&self) -> Result<(), ApiError> {
        let a = &self.attrs;
        let allowed = |ok: bool, what: &str| if ok { Ok(()) } else { Err(bad(what)) };
        match self.kind {
            BlockKind::Heading => {
                allowed(
                    matches!(a.level, Some(1..=3)),
                    "a heading needs level 1 to 3",
                )?;
                allowed(
                    a.indent.is_none()
                        && a.checked.is_none()
                        && a.language.is_none()
                        && a.tone.is_none(),
                    "unexpected heading attribute",
                )?;
            }
            BlockKind::BulletItem | BlockKind::NumberedItem | BlockKind::TodoItem => {
                allowed(
                    a.indent.is_none_or(|i| i <= MAX_INDENT),
                    "indent is too deep",
                )?;
                allowed(
                    a.level.is_none() && a.language.is_none() && a.tone.is_none(),
                    "unexpected list attribute",
                )?;
                allowed(
                    self.kind == BlockKind::TodoItem || a.checked.is_none(),
                    "only to-do items can be checked",
                )?;
            }
            BlockKind::Code => {
                allowed(
                    a.language.as_ref().is_none_or(|l| l.len() <= 30),
                    "language is too long",
                )?;
                allowed(
                    a.level.is_none()
                        && a.indent.is_none()
                        && a.checked.is_none()
                        && a.tone.is_none(),
                    "unexpected code attribute",
                )?;
            }
            BlockKind::Callout => {
                let tone_ok = a
                    .tone
                    .as_deref()
                    .is_none_or(|t| ["info", "warning", "success", "danger"].contains(&t));
                allowed(tone_ok, "unknown callout tone")?;
                allowed(
                    a.level.is_none()
                        && a.indent.is_none()
                        && a.checked.is_none()
                        && a.language.is_none(),
                    "unexpected callout attribute",
                )?;
            }
            BlockKind::Paragraph | BlockKind::Quote | BlockKind::Divider => {
                allowed(*a == Attrs::default(), "this block takes no attributes")?;
            }
        }
        if self.kind == BlockKind::Divider {
            allowed(self.spans.is_empty(), "a divider has no content")?;
        }
        if self.kind == BlockKind::Code {
            allowed(
                self.spans.len() <= 1
                    && self
                        .spans
                        .iter()
                        .all(|s| s.text.is_some() && s.marks.is_empty() && s.link.is_none()),
                "code is one plain text span",
            )?;
        }
        allowed(
            self.spans.len() <= MAX_SPANS_PER_BLOCK,
            "too many spans in a block",
        )?;
        let mut text_len = 0;
        for span in &self.spans {
            let kinds = [
                span.text.is_some(),
                span.mention.is_some(),
                span.page_link.is_some(),
            ];
            allowed(
                kinds.iter().filter(|k| **k).count() == 1,
                "a span is text, a mention or a page link",
            )?;
            if let Some(text) = &span.text {
                text_len += text.chars().count();
            } else {
                allowed(
                    span.marks.is_empty() && span.link.is_none(),
                    "only text can be styled or linked",
                )?;
            }
            if let Some(link) = &span.link {
                allowed(
                    safe_link(link),
                    "link is empty, too long or uses a script scheme",
                )?;
            }
            if let Some(m) = &span.mention {
                allowed(m.label.chars().count() <= 300, "mention label is too long")?;
            }
            if let Some(p) = &span.page_link {
                allowed(
                    p.label.chars().count() <= 300,
                    "page link label is too long",
                )?;
            }
        }
        allowed(text_len <= MAX_TEXT_PER_BLOCK, "a block's text is too long")?;
        if self.kind.is_list_item() || self.kind == BlockKind::Heading {
            // Lists and headings are single-line content; the editor never produces newlines there.
            allowed(
                self.spans
                    .iter()
                    .all(|s| s.text.as_ref().is_none_or(|t| !t.contains('\n'))),
                "this block is a single line",
            )?;
        }
        Ok(())
    }

    /// The block's text without formatting (search, previews). Mentions and links contribute their label.
    pub fn plain_text(&self) -> String {
        self.spans
            .iter()
            .map(|s| {
                s.text
                    .clone()
                    .or_else(|| s.mention.as_ref().map(|m| m.label.clone()))
                    .or_else(|| s.page_link.as_ref().map(|p| p.label.clone()))
                    .unwrap_or_default()
            })
            .collect()
    }
}

/// Validate a whole document: the count, and every block.
pub fn validate_blocks(blocks: &[BlockData]) -> Result<(), ApiError> {
    if blocks.len() > MAX_BLOCKS {
        return Err(bad("this page has too many blocks"));
    }
    blocks.iter().try_for_each(BlockData::validate)
}
