import { getSchema } from "@tiptap/core";
import { Node as PMNode } from "@tiptap/pm/model";
import { prosemirrorToYXmlFragment } from "@tiptap/y-tiptap";
import type { QueryClient } from "@tanstack/react-query";
import type { BlockData } from "woodmouse-client";
import * as Y from "yjs";
import { describe, expect, it } from "vitest";

import { fromBlocks, toBlocks } from "./editorBlocks";
import { editorExtensions } from "../components/wiki/editor/extensions";

/**
 * The server turns the editor's live Yjs document into blocks (backend/src/live/convert.rs). These
 * fixtures are documents written by the real editor schema, with the blocks the editor's own save
 * produces for them; the backend test reads the same file, so the two sides cannot drift apart.
 *
 * Regenerate with `pnpm vitest run -u src/lib/yjsFixtures.test.ts` (the file is a vitest file snapshot).
 */
const FIXTURES = "../../../backend/tests/fixtures/editor_docs.json";

const schema = getSchema(
  editorExtensions({ workspaceId: "x", queryClient: {} as QueryClient, readOnly: true }),
);

const ID1 = "00000000-0000-0000-0000-000000000001";
const ID2 = "00000000-0000-0000-0000-000000000002";

const docs: Record<string, BlockData[]> = {
  everything: [
    { type: "heading", attrs: { level: 2 }, spans: [{ text: "Plan" }] },
    {
      type: "paragraph",
      attrs: {},
      spans: [
        { text: "plain " },
        { text: "bold italic", marks: ["bold", "italic"] },
        { text: " site", marks: ["bold"], link: "https://example.com/a" },
        { mention: { task_id: ID1, label: "Order oak" } },
        { page_link: { page_id: ID2, label: "Budget" } },
        { text: "line1\nline2" },
        { text: " mono", marks: ["code"] },
        { text: " gone", marks: ["strike"] },
      ],
    },
    { type: "todo_item", attrs: { checked: true, indent: 1 }, spans: [{ text: "done" }] },
    { type: "todo_item", attrs: { checked: false }, spans: [{ text: "open" }] },
    { type: "bullet_item", attrs: {}, spans: [{ text: "a" }] },
    { type: "bullet_item", attrs: { indent: 2 }, spans: [{ text: "deep" }] },
    { type: "numbered_item", attrs: {}, spans: [{ text: "one" }] },
    { type: "quote", attrs: {}, spans: [{ text: "quoted" }] },
    { type: "code", attrs: { language: "rust" }, spans: [{ text: "fn main() {\n}" }] },
    { type: "code", attrs: {}, spans: [{ text: "plain code" }] },
    { type: "callout", attrs: { tone: "danger" }, spans: [{ text: "careful" }] },
    { type: "divider", attrs: {} },
  ],
  empty_paragraphs_in_the_middle: [
    { type: "paragraph", attrs: {}, spans: [{ text: "before" }] },
    { type: "paragraph", attrs: {}, spans: [] },
    { type: "paragraph", attrs: {}, spans: [{ text: "after" }] },
  ],
  empty_page: [],
  unusual_link_formats: [
    {
      type: "paragraph",
      attrs: {},
      spans: [
        { text: "app", link: "obsidian://open?vault=x" },
        { text: " mail", link: "mailto:a@b.c" },
        { text: " relative", link: "/w/1/wiki/2" },
      ],
    },
  ],
};

/** What the editor would save for these blocks: its own document, trailing empty paragraph removed. */
function expectedBlocks(blocks: BlockData[]): BlockData[] {
  const saved = toBlocks(fromBlocks(blocks));
  while (
    saved.length &&
    saved[saved.length - 1].type === "paragraph" &&
    !saved[saved.length - 1].spans?.length
  )
    saved.pop();
  return saved;
}

/** The document the collaboration extension would write for these blocks (fixed client id: stable bytes). */
function liveDocument(blocks: BlockData[]): Y.Doc {
  const ydoc = new Y.Doc();
  ydoc.clientID = 1;
  prosemirrorToYXmlFragment(
    PMNode.fromJSON(schema, fromBlocks(blocks)),
    ydoc.getXmlFragment("default"),
  );
  return ydoc;
}

describe("yjs fixtures for the server", () => {
  it("match what the editor produces today", async () => {
    const fixtures = Object.entries(docs).map(([name, blocks]) => ({
      name,
      update_base64: btoa(String.fromCharCode(...Y.encodeStateAsUpdate(liveDocument(blocks)))),
      blocks: expectedBlocks(blocks),
    }));
    await expect(JSON.stringify(fixtures, null, 2) + "\n").toMatchFileSnapshot(FIXTURES);
  });
});
