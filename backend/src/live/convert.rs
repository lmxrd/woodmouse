//! Converts between a page's blocks and the live editing document.
//!
//! The live document is a Yjs `XmlFragment` named `default`, laid out the way the editor's
//! collaboration extension writes it (y-prosemirror): one element per block named after the editor's node
//! type, node attributes as element attributes, text as `XmlText` runs whose formatting attributes are the
//! marks, and mentions, page links and line breaks as inline child elements. Blocks stay the stored form;
//! this only runs when a room is created (blocks to document) and when its edits are saved (document to
//! blocks).

use std::collections::HashMap;
use std::sync::Arc;

use yrs::types::Attrs;
use yrs::types::text::YChange;
use yrs::types::xml::{Xml, XmlFragment, XmlOut};
use yrs::{
    Any, Doc, Out, ReadTxn, Text, Transact, WriteTxn, XmlElementPrelim, XmlElementRef,
    XmlTextPrelim,
};

use crate::blocks::{Attrs as BlockAttrs, BlockData, BlockKind, Mark, PageLink, Span, TaskMention};

/// The name of the fragment the editor's collaboration extension syncs.
pub const FRAGMENT: &str = "default";

const MARK_ORDER: [(&str, Mark); 4] = [
    ("bold", Mark::Bold),
    ("italic", Mark::Italic),
    ("strike", Mark::Strike),
    ("code", Mark::Code),
];

fn node_kind(tag: &str) -> Option<BlockKind> {
    Some(match tag {
        "paragraph" => BlockKind::Paragraph,
        "heading" => BlockKind::Heading,
        "bulletItem" => BlockKind::BulletItem,
        "numberedItem" => BlockKind::NumberedItem,
        "todoItem" => BlockKind::TodoItem,
        "codeBlock" => BlockKind::Code,
        "quote" => BlockKind::Quote,
        "callout" => BlockKind::Callout,
        "divider" => BlockKind::Divider,
        _ => return None,
    })
}

fn node_tag(kind: BlockKind) -> &'static str {
    match kind {
        BlockKind::Paragraph => "paragraph",
        BlockKind::Heading => "heading",
        BlockKind::BulletItem => "bulletItem",
        BlockKind::NumberedItem => "numberedItem",
        BlockKind::TodoItem => "todoItem",
        BlockKind::Code => "codeBlock",
        BlockKind::Quote => "quote",
        BlockKind::Callout => "callout",
        BlockKind::Divider => "divider",
    }
}

fn any_i64(out: Option<Out>) -> Option<i64> {
    match out? {
        Out::Any(Any::Number(n)) => n.as_i64(),
        _ => None,
    }
}

fn any_string(out: Option<Out>) -> Option<String> {
    match out? {
        Out::Any(Any::String(s)) => Some(s.to_string()),
        _ => None,
    }
}

fn any_bool(out: Option<Out>) -> Option<bool> {
    match out? {
        Out::Any(Any::Bool(b)) => Some(b),
        _ => None,
    }
}

/// Merge neighbouring text with the same formatting, as the editor's own save does.
fn push_text(spans: &mut Vec<Span>, text: &str, marks: Vec<Mark>, link: Option<String>) {
    if text.is_empty() {
        return;
    }
    if let Some(last) = spans.last_mut()
        && last.text.is_some()
        && last.marks == marks
        && last.link == link
    {
        if let Some(existing) = &mut last.text {
            existing.push_str(text);
        }
        return;
    }
    spans.push(Span {
        text: Some(text.to_string()),
        marks,
        link,
        ..Default::default()
    });
}

fn text_attrs(attrs: Option<&Attrs>) -> (Vec<Mark>, Option<String>) {
    let Some(attrs) = attrs else {
        return (vec![], None);
    };
    let marks = MARK_ORDER
        .iter()
        .filter(|(name, _)| attrs.contains_key(*name))
        .map(|(_, mark)| *mark)
        .collect();
    let link = match attrs.get("link") {
        Some(Any::Map(map)) => match map.get("href") {
            Some(Any::String(href)) => Some(href.to_string()),
            _ => None,
        },
        _ => None,
    };
    (marks, link)
}

fn block_of<T: ReadTxn>(txn: &T, element: &XmlElementRef) -> Option<BlockData> {
    let kind = node_kind(element.tag())?;
    let mut attrs = BlockAttrs::default();
    match kind {
        BlockKind::Heading => {
            attrs.level = Some(any_i64(element.get_attribute(txn, "level")).unwrap_or(1) as u8)
        }
        BlockKind::BulletItem | BlockKind::NumberedItem | BlockKind::TodoItem => {
            attrs.indent = any_i64(element.get_attribute(txn, "indent"))
                .filter(|n| *n > 0)
                .map(|n| n as u8);
            if kind == BlockKind::TodoItem {
                attrs.checked =
                    Some(any_bool(element.get_attribute(txn, "checked")).unwrap_or(false));
            }
        }
        BlockKind::Code => attrs.language = any_string(element.get_attribute(txn, "language")),
        BlockKind::Callout => {
            attrs.tone = Some(
                any_string(element.get_attribute(txn, "tone")).unwrap_or_else(|| "info".into()),
            )
        }
        _ => {}
    }
    if kind == BlockKind::Divider {
        return Some(BlockData {
            kind,
            attrs,
            spans: vec![],
        });
    }

    let mut spans: Vec<Span> = vec![];
    for child in element.children(txn) {
        match child {
            XmlOut::Text(text) => {
                for diff in text.diff(txn, YChange::identity) {
                    let Out::Any(Any::String(s)) = &diff.insert else {
                        continue;
                    };
                    if kind == BlockKind::Code {
                        // Code is plain text, whatever formatting was applied.
                        push_text(&mut spans, s, vec![], None);
                    } else {
                        let (marks, link) = text_attrs(diff.attributes.as_deref());
                        push_text(&mut spans, s, marks, link);
                    }
                }
            }
            XmlOut::Element(inline) => match inline.tag().as_ref() {
                "hardBreak" => push_text(&mut spans, "\n", vec![], None),
                "taskMention" if kind != BlockKind::Code => {
                    if let Some(task_id) = any_string(inline.get_attribute(txn, "id"))
                        .and_then(|s| uuid::Uuid::parse_str(&s).ok())
                    {
                        spans.push(Span {
                            mention: Some(TaskMention {
                                task_id,
                                label: any_string(inline.get_attribute(txn, "label"))
                                    .unwrap_or_default(),
                            }),
                            ..Default::default()
                        });
                    }
                }
                "pageLink" if kind != BlockKind::Code => {
                    if let Some(page_id) = any_string(inline.get_attribute(txn, "id"))
                        .and_then(|s| uuid::Uuid::parse_str(&s).ok())
                    {
                        spans.push(Span {
                            page_link: Some(PageLink {
                                page_id,
                                label: any_string(inline.get_attribute(txn, "label"))
                                    .unwrap_or_default(),
                            }),
                            ..Default::default()
                        });
                    }
                }
                _ => {}
            },
            XmlOut::Fragment(_) => {}
        }
    }
    // Lists and headings are one line; a line break inside one becomes a space.
    if kind.is_list_item() || kind == BlockKind::Heading {
        for span in &mut spans {
            if let Some(text) = &mut span.text {
                *text = text.replace('\n', " ");
            }
        }
    }
    Some(BlockData { kind, attrs, spans })
}

/// The blocks the document currently holds. Trailing empty paragraphs (the editor keeps one after a
/// divider or code block so there is somewhere to click) are not content and are left out.
pub fn blocks_from_doc(doc: &Doc) -> Vec<BlockData> {
    let txn = doc.transact();
    let Some(fragment) = txn.get_xml_fragment(FRAGMENT) else {
        return vec![];
    };
    let mut blocks: Vec<BlockData> = fragment
        .children(&txn)
        .filter_map(|node| match node {
            XmlOut::Element(element) => block_of(&txn, &element),
            _ => None,
        })
        .collect();
    crate::blocks::trim_trailing_empty(&mut blocks);
    blocks
}

fn any_marks(marks: &[Mark], link: &Option<String>) -> Option<Attrs> {
    if marks.is_empty() && link.is_none() {
        return None;
    }
    let mut attrs: Attrs = HashMap::new();
    for (name, mark) in MARK_ORDER {
        if marks.contains(&mark) {
            attrs.insert(Arc::from(name), Any::Map(Arc::new(HashMap::new())));
        }
    }
    if let Some(href) = link {
        let mut map = HashMap::new();
        map.insert("href".to_string(), Any::String(Arc::from(href.as_str())));
        attrs.insert(Arc::from("link"), Any::Map(Arc::new(map)));
    }
    Some(attrs)
}

/// Write `blocks` into the document's fragment (which must be empty).
pub fn seed_doc(doc: &Doc, blocks: &[BlockData]) {
    let mut txn = doc.transact_mut();
    let fragment = txn.get_or_insert_xml_fragment(FRAGMENT);
    let empty = [BlockData {
        kind: BlockKind::Paragraph,
        attrs: BlockAttrs::default(),
        spans: vec![],
    }];
    let blocks = if blocks.is_empty() {
        &empty[..]
    } else {
        blocks
    };
    for block in blocks {
        let element = fragment.push_back(&mut txn, XmlElementPrelim::empty(node_tag(block.kind)));
        let a = &block.attrs;
        if let Some(level) = a.level {
            element.insert_attribute(&mut txn, "level", i64::from(level));
        }
        if let Some(indent) = a.indent {
            element.insert_attribute(&mut txn, "indent", i64::from(indent));
        }
        if let Some(checked) = a.checked {
            element.insert_attribute(&mut txn, "checked", checked);
        }
        if let Some(language) = &a.language {
            element.insert_attribute(&mut txn, "language", language.as_str());
        }
        if let Some(tone) = &a.tone {
            element.insert_attribute(&mut txn, "tone", tone.as_str());
        }

        // Consecutive text goes into one XmlText with the marks as formatting, like the editor does.
        let mut run: Option<yrs::XmlTextRef> = None;
        for span in &block.spans {
            if let Some(text) = &span.text {
                for (i, line) in text.split('\n').enumerate() {
                    if i > 0 {
                        element.push_back(&mut txn, XmlElementPrelim::empty("hardBreak"));
                        run = None;
                    }
                    if line.is_empty() {
                        continue;
                    }
                    let target = match &run {
                        Some(t) => t.clone(),
                        None => {
                            let t = element.push_back(&mut txn, XmlTextPrelim::new(""));
                            run = Some(t.clone());
                            t
                        }
                    };
                    let at = target.len(&txn);
                    match any_marks(&span.marks, &span.link) {
                        Some(attrs) => target.insert_with_attributes(&mut txn, at, line, attrs),
                        None => target.insert(&mut txn, at, line),
                    }
                }
                continue;
            }
            run = None;
            let (tag, id, label) = if let Some(m) = &span.mention {
                ("taskMention", m.task_id, &m.label)
            } else if let Some(p) = &span.page_link {
                ("pageLink", p.page_id, &p.label)
            } else {
                continue;
            };
            let inline = element.push_back(&mut txn, XmlElementPrelim::empty(tag));
            inline.insert_attribute(&mut txn, "id", id.to_string());
            inline.insert_attribute(&mut txn, "label", label.as_str());
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::blocks::validate_blocks;

    fn text(t: &str, marks: Vec<Mark>) -> Span {
        Span {
            text: Some(t.into()),
            marks,
            ..Default::default()
        }
    }

    pub fn sample() -> Vec<BlockData> {
        let block = |kind, attrs: BlockAttrs, spans: Vec<Span>| BlockData { kind, attrs, spans };
        vec![
            block(
                BlockKind::Heading,
                BlockAttrs {
                    level: Some(2),
                    ..Default::default()
                },
                vec![text("Plan", vec![])],
            ),
            block(
                BlockKind::Paragraph,
                BlockAttrs::default(),
                vec![
                    text("plain ", vec![]),
                    text("bold italic", vec![Mark::Bold, Mark::Italic]),
                    Span {
                        text: Some(" site".into()),
                        marks: vec![Mark::Bold],
                        link: Some("https://example.com/a".into()),
                        ..Default::default()
                    },
                    Span {
                        mention: Some(TaskMention {
                            task_id: uuid::Uuid::from_u128(1),
                            label: "Order oak".into(),
                        }),
                        ..Default::default()
                    },
                    Span {
                        page_link: Some(PageLink {
                            page_id: uuid::Uuid::from_u128(2),
                            label: "Budget".into(),
                        }),
                        ..Default::default()
                    },
                    text("line1\nline2", vec![]),
                    text(" mono", vec![Mark::Code]),
                ],
            ),
            block(
                BlockKind::TodoItem,
                BlockAttrs {
                    checked: Some(true),
                    indent: Some(1),
                    ..Default::default()
                },
                vec![text("done", vec![])],
            ),
            block(
                BlockKind::BulletItem,
                BlockAttrs::default(),
                vec![text("a", vec![])],
            ),
            block(
                BlockKind::Code,
                BlockAttrs {
                    language: Some("rust".into()),
                    ..Default::default()
                },
                vec![text("fn main() {\n}", vec![])],
            ),
            block(
                BlockKind::Callout,
                BlockAttrs {
                    tone: Some("warning".into()),
                    ..Default::default()
                },
                vec![text("careful", vec![])],
            ),
            block(BlockKind::Divider, BlockAttrs::default(), vec![]),
        ]
    }

    #[test]
    fn blocks_survive_a_trip_through_the_live_document() {
        let blocks = sample();
        let doc = Doc::new();
        seed_doc(&doc, &blocks);
        let back = blocks_from_doc(&doc);
        assert_eq!(back, blocks);
        assert!(validate_blocks(&back).is_ok());
    }

    #[test]
    fn an_empty_page_is_one_empty_paragraph_in_the_document_and_no_blocks_back() {
        let doc = Doc::new();
        seed_doc(&doc, &[]);
        assert_eq!(blocks_from_doc(&doc), vec![]);
    }

    #[test]
    fn line_breaks_in_single_line_blocks_become_spaces() {
        let doc = Doc::new();
        let blocks = vec![BlockData {
            kind: BlockKind::Heading,
            attrs: BlockAttrs {
                level: Some(1),
                ..Default::default()
            },
            spans: vec![text("a\nb", vec![])],
        }];
        seed_doc(&doc, &blocks);
        let back = blocks_from_doc(&doc);
        assert_eq!(back[0].spans[0].text.as_deref(), Some("a b"));
        assert!(validate_blocks(&back).is_ok());
    }
}
