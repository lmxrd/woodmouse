import type { BlockData } from "woodmouse-client";
import { describe, expect, it } from "vitest";

import { fromBlocks, toBlocks } from "./editorBlocks";

const blocks: BlockData[] = [
  { type: "heading", attrs: { level: 2 }, spans: [{ text: "Title" }] },
  {
    type: "paragraph",
    attrs: {},
    spans: [
      { text: "plain " },
      { text: "bold", marks: ["bold", "italic"] },
      { text: " site", link: "https://example.com", marks: ["bold"] },
      { mention: { task_id: "t1", label: "Order oak" } },
      { page_link: { page_id: "p1", label: "Budget" } },
      { text: "line1\nline2" },
    ],
  },
  { type: "todo_item", attrs: { checked: true, indent: 1 }, spans: [{ text: "done" }] },
  { type: "bullet_item", attrs: {}, spans: [{ text: "a" }] },
  { type: "code", attrs: { language: "rust" }, spans: [{ text: "fn main() {}\n" }] },
  { type: "callout", attrs: { tone: "warning" }, spans: [{ text: "careful" }] },
  { type: "divider", attrs: {} },
  { type: "paragraph", attrs: {}, spans: [] },
];

describe("editor <-> blocks", () => {
  it("round trips every block type", () => {
    expect(toBlocks(fromBlocks(blocks))).toEqual(blocks);
  });

  it("merges adjacent text with the same marks and drops unknown nodes", () => {
    const doc = {
      type: "doc",
      content: [
        {
          type: "paragraph",
          content: [
            { type: "text", text: "a", marks: [{ type: "bold" }] },
            { type: "text", text: "b", marks: [{ type: "bold" }] },
          ],
        },
        { type: "somethingElse" },
      ],
    };
    expect(toBlocks(doc)).toEqual([
      { type: "paragraph", attrs: {}, spans: [{ text: "ab", marks: ["bold"] }] },
    ]);
  });

  it("opens an empty page as one empty paragraph", () => {
    expect(fromBlocks([])).toEqual({ type: "doc", content: [{ type: "paragraph" }] });
  });
});
