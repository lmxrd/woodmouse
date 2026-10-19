import type { JSONContent } from "@tiptap/core";
import type { Attrs, BlockData, BlockKind, Mark, Span } from "woodmouse-sdk";

/**
 * The editor is only a view of the page: the stored form is a flat list of blocks (see the backend's
 * `blocks.rs`). These two functions convert between the two, and `fromBlocks(toBlocks(doc))` is stable.
 */

const NODE_OF: Record<BlockKind, string> = {
  paragraph: "paragraph",
  heading: "heading",
  bullet_item: "bulletItem",
  numbered_item: "numberedItem",
  todo_item: "todoItem",
  code: "codeBlock",
  quote: "quote",
  callout: "callout",
  divider: "divider",
  file: "fileBlock",
};
const KIND_OF = Object.fromEntries(Object.entries(NODE_OF).map(([k, v]) => [v, k])) as Record<
  string,
  BlockKind
>;

const MARK_ORDER: Mark[] = ["bold", "italic", "strike", "code"];
const MARK_NODE: Record<string, Mark> = {
  bold: "bold",
  italic: "italic",
  strike: "strike",
  code: "code",
};

function sameMarks(a: Span, b: Span) {
  return (a.link ?? null) === (b.link ?? null) && (a.marks ?? []).join() === (b.marks ?? []).join();
}

function spansOf(content: JSONContent[] | undefined): Span[] {
  const spans: Span[] = [];
  const addText = (text: string, span: Omit<Span, "text">) => {
    const last = spans[spans.length - 1];
    if (last?.text != null && sameMarks(last, span)) last.text += text;
    else spans.push({ ...span, text });
  };
  for (const node of content ?? []) {
    if (node.type === "text" && node.text) {
      const marks = MARK_ORDER.filter((m) => node.marks?.some((n) => MARK_NODE[n.type] === m));
      const link = node.marks?.find((m) => m.type === "link")?.attrs?.href as string | undefined;
      addText(node.text, { ...(marks.length ? { marks } : {}), ...(link ? { link } : {}) });
    } else if (node.type === "hardBreak") {
      addText("\n", {});
    } else if (node.type === "taskMention") {
      spans.push({ mention: { task_id: node.attrs?.id, label: node.attrs?.label ?? "" } });
    } else if (node.type === "pageLink") {
      spans.push({ page_link: { page_id: node.attrs?.id, label: node.attrs?.label ?? "" } });
    }
  }
  return spans;
}

/** Editor document -> blocks to save. */
export function toBlocks(doc: JSONContent): BlockData[] {
  return (doc.content ?? []).flatMap((node): BlockData[] => {
    const type = KIND_OF[node.type ?? ""];
    if (!type) return [];
    const a = node.attrs ?? {};
    const attrs: Attrs = {};
    if (type === "heading") attrs.level = a.level;
    if (type === "bullet_item" || type === "numbered_item" || type === "todo_item") {
      if (a.indent) attrs.indent = a.indent;
    }
    if (type === "todo_item") attrs.checked = !!a.checked;
    if (type === "code" && a.language) attrs.language = a.language;
    if (type === "callout") attrs.tone = a.tone ?? "info";
    if (type === "divider") return [{ type, attrs }];
    if (type === "file") {
      if (!a.fileId) return [];
      attrs.file_id = a.fileId;
      if (a.label) attrs.label = a.label;
      return [{ type, attrs, spans: [] }];
    }
    if (type === "code") {
      // Code is one plain text span, never marked.
      const text = (node.content ?? []).map((n) => n.text ?? "").join("");
      return [{ type, attrs, spans: text ? [{ text }] : [] }];
    }
    return [{ type, attrs, spans: spansOf(node.content) }];
  });
}

function inlineOf(spans: Span[] | undefined): JSONContent[] {
  const out: JSONContent[] = [];
  for (const span of spans ?? []) {
    if (span.mention) {
      out.push({
        type: "taskMention",
        attrs: { id: span.mention.task_id, label: span.mention.label },
      });
    } else if (span.page_link) {
      out.push({
        type: "pageLink",
        attrs: { id: span.page_link.page_id, label: span.page_link.label },
      });
    } else if (span.text) {
      const marks: NonNullable<JSONContent["marks"]> = (span.marks ?? []).map((type) => ({ type }));
      if (span.link) marks.push({ type: "link", attrs: { href: span.link } });
      span.text.split("\n").forEach((line, i) => {
        if (i > 0) out.push({ type: "hardBreak" });
        if (line) out.push({ type: "text", text: line, ...(marks.length ? { marks } : {}) });
      });
    }
  }
  return out;
}

/** Stored blocks -> editor document. */
export function fromBlocks(blocks: BlockData[]): JSONContent {
  const content = blocks.map((block): JSONContent => {
    const attrs = block.attrs ?? {};
    const type = NODE_OF[block.type];
    // Code keeps its newlines inside one text node; everywhere else they are line breaks.
    const children =
      block.type === "code"
        ? (block.spans ?? []).flatMap((s) => (s.text ? [{ type: "text", text: s.text }] : []))
        : inlineOf(block.spans);
    const node: JSONContent = { type };
    if (block.type === "heading") node.attrs = { level: attrs.level ?? 1 };
    else if (block.type === "todo_item")
      node.attrs = { indent: attrs.indent ?? 0, checked: !!attrs.checked };
    else if (block.type === "bullet_item" || block.type === "numbered_item")
      node.attrs = { indent: attrs.indent ?? 0 };
    else if (block.type === "code") node.attrs = { language: attrs.language ?? null };
    else if (block.type === "callout") node.attrs = { tone: attrs.tone ?? "info" };
    else if (block.type === "file")
      node.attrs = { fileId: attrs.file_id ?? null, label: attrs.label ?? "" };
    if (children.length) node.content = children;
    return node;
  });
  return { type: "doc", content: content.length ? content : [{ type: "paragraph" }] };
}
