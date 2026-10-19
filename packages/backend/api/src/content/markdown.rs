//! Markdown export of pages (the stored block format is the source of truth; this is one-way).

use crate::blocks::{BlockData, BlockKind, Mark, Span};

fn escape(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    for c in text.chars() {
        if matches!(c, '\\' | '*' | '_' | '`' | '[' | ']' | '<' | '~') {
            out.push('\\');
        }
        out.push(c);
    }
    out
}

/// Wrap `text` in the markers of each mark, keeping leading and trailing spaces outside them.
fn mark(text: &str, marks: &[Mark]) -> String {
    let trimmed = text.trim();
    if trimmed.is_empty() {
        return text.to_string();
    }
    let start = &text[..text.len() - text.trim_start().len()];
    let end = &text[text.trim_end().len()..];
    let mut inner = if marks.contains(&Mark::Code) {
        format!("`{trimmed}`")
    } else {
        escape(trimmed)
    };
    if marks.contains(&Mark::Strike) {
        inner = format!("~~{inner}~~");
    }
    if marks.contains(&Mark::Italic) {
        inner = format!("*{inner}*");
    }
    if marks.contains(&Mark::Bold) {
        inner = format!("**{inner}**");
    }
    format!("{start}{inner}{end}")
}

/// `page_href` turns a linked page's id into a link target (a relative file path in a whole-wiki
/// export); when it returns `None` the page link is written as plain text.
fn inline(spans: &[Span], page_href: &dyn Fn(uuid::Uuid) -> Option<String>) -> String {
    spans
        .iter()
        .map(|s| {
            if let Some(m) = &s.mention {
                format!("@{}", escape(&m.label))
            } else if let Some(p) = &s.page_link {
                match page_href(p.page_id) {
                    Some(href) => format!("[{}]({href})", escape(&p.label)),
                    None => escape(&p.label),
                }
            } else {
                let text = mark(s.text.as_deref().unwrap_or(""), &s.marks);
                match &s.link {
                    Some(link) => {
                        format!("[{text}]({})", link.replace(')', "%29").replace(' ', "%20"))
                    }
                    None => text,
                }
            }
        })
        .collect()
}

/// The page as Markdown: its title as a heading, then the blocks.
pub fn page_to_markdown(
    title: &str,
    blocks: &[BlockData],
    page_href: &dyn Fn(uuid::Uuid) -> Option<String>,
) -> String {
    let mut out = format!("# {}\n", escape(title));
    let mut counters: Vec<usize> = vec![];
    // Whether the previous block was a numbered item (Some(true)), another list item or not a list.
    let mut previous_list: Option<bool> = None;
    for block in blocks {
        let indent = block.attrs.indent.unwrap_or(0) as usize;
        let pad = "  ".repeat(indent);
        let text = inline(&block.spans, page_href);
        let is_list = block.kind.is_list_item();
        // Items of one list stay tight; everything else, including a switch between numbered and
        // bulleted lists, is a paragraph apart.
        let numbered = block.kind == BlockKind::NumberedItem;
        if !(is_list && previous_list == Some(numbered)) {
            out.push('\n');
        }
        previous_list = is_list.then_some(numbered);
        match block.kind {
            BlockKind::Paragraph => out.push_str(&format!("{}\n", text.replace('\n', "  \n"))),
            BlockKind::Heading => {
                let level = block.attrs.level.unwrap_or(1).clamp(1, 3) as usize + 1;
                out.push_str(&format!("{} {text}\n", "#".repeat(level)));
            }
            BlockKind::BulletItem => out.push_str(&format!("{pad}- {text}\n")),
            BlockKind::NumberedItem => {
                counters.truncate(indent + 1);
                counters.resize(indent + 1, 0);
                counters[indent] += 1;
                out.push_str(&format!("{pad}{}. {text}\n", counters[indent]));
            }
            BlockKind::TodoItem => {
                let mark = if block.attrs.checked == Some(true) {
                    "x"
                } else {
                    " "
                };
                out.push_str(&format!("{pad}- [{mark}] {text}\n"));
            }
            BlockKind::Code => {
                let code: String = block
                    .spans
                    .iter()
                    .filter_map(|s| s.text.as_deref())
                    .collect();
                let lang = block.attrs.language.as_deref().unwrap_or("");
                let fence = if code.contains("```") { "````" } else { "```" };
                out.push_str(&format!("{fence}{lang}\n{code}\n{fence}\n"));
            }
            BlockKind::Quote => out.push_str(&format!("> {}\n", text.replace('\n', "\n> "))),
            BlockKind::Callout => {
                let kind = match block.attrs.tone.as_deref() {
                    Some("warning") => "WARNING",
                    Some("success") => "TIP",
                    Some("danger") => "CAUTION",
                    _ => "NOTE",
                };
                out.push_str(&format!("> [!{kind}]\n> {}\n", text.replace('\n', "\n> ")));
            }
            BlockKind::Divider => out.push_str("---\n"),
            // The file itself is not part of the export; its name is.
            BlockKind::File => out.push_str(&format!(
                "*Attachment: {}*\n",
                escape(block.attrs.label.as_deref().unwrap_or("file"))
            )),
        }
        if !is_list {
            counters.clear();
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::blocks::Attrs;

    fn text(t: &str, marks: Vec<Mark>) -> Span {
        Span {
            text: Some(t.into()),
            marks,
            ..Default::default()
        }
    }

    #[test]
    fn exports_every_block_type() {
        let block = |kind, attrs: Attrs, spans: Vec<Span>| BlockData { kind, attrs, spans };
        let blocks = vec![
            block(
                BlockKind::Heading,
                Attrs {
                    level: Some(1),
                    ..Default::default()
                },
                vec![text("Plan", vec![])],
            ),
            block(
                BlockKind::Paragraph,
                Attrs::default(),
                vec![
                    text("a ", vec![]),
                    text("bold", vec![Mark::Bold]),
                    text(" *star*", vec![]),
                ],
            ),
            block(
                BlockKind::BulletItem,
                Attrs::default(),
                vec![text("one", vec![])],
            ),
            block(
                BlockKind::BulletItem,
                Attrs {
                    indent: Some(1),
                    ..Default::default()
                },
                vec![text("nested", vec![])],
            ),
            block(
                BlockKind::NumberedItem,
                Attrs::default(),
                vec![text("x", vec![])],
            ),
            block(
                BlockKind::NumberedItem,
                Attrs::default(),
                vec![text("y", vec![])],
            ),
            block(
                BlockKind::TodoItem,
                Attrs {
                    checked: Some(true),
                    ..Default::default()
                },
                vec![text("done", vec![])],
            ),
            block(
                BlockKind::Callout,
                Attrs {
                    tone: Some("warning".into()),
                    ..Default::default()
                },
                vec![text("careful", vec![])],
            ),
            block(
                BlockKind::Code,
                Attrs {
                    language: Some("rust".into()),
                    ..Default::default()
                },
                vec![text("fn main() {}", vec![])],
            ),
            block(BlockKind::Divider, Attrs::default(), vec![]),
        ];
        let md = page_to_markdown("My page", &blocks, &|_| None);
        assert_eq!(
            md,
            "# My page\n\n## Plan\n\na **bold** \\*star\\*\n\n- one\n  - nested\n\n1. x\n2. y\n\n- [x] done\n\n> [!WARNING]\n> careful\n\n```rust\nfn main() {}\n```\n\n---\n"
        );
    }
}
