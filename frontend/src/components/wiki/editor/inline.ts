import { Extension, Node } from "@tiptap/core";
import type { Editor } from "@tiptap/core";

/** The current title of a page or task a mention or link points at, and whether it is in the trash. */
export interface RefInfo {
  title: string;
  deleted: boolean;
}

declare module "@tiptap/core" {
  interface Storage {
    references: ReferenceStorage;
  }
}

interface ReferenceStorage {
  pages: Map<string, RefInfo>;
  tasks: Map<string, RefInfo>;
  /** Repaint callbacks of the mentions and links currently shown. */
  paint: Set<() => void>;
}

/**
 * Where mentions and links look up their target's current title. It lives beside the document, not in
 * it: the document only holds ids (and the label from when the link was made, as a fallback), so renames
 * and deletions show up without anything being rewritten or synced.
 */
export const References = Extension.create<unknown, ReferenceStorage>({
  name: "references",
  addStorage() {
    return { pages: new Map(), tasks: new Map(), paint: new Set() };
  },
});

/** Give the editor's mentions and links the current titles of their targets. */
export function setReferences(
  editor: Editor,
  found: { pages: Map<string, RefInfo>; tasks: Map<string, RefInfo> },
) {
  const storage = editor.storage.references;
  storage.pages = found.pages;
  storage.tasks = found.tasks;
  storage.paint.forEach((paint) => paint());
}

/** The ids of the pages and tasks the editor's document mentions or links to. */
export function referencedInEditor(editor: Editor): { pageIds: string[]; taskIds: string[] } {
  const pages = new Set<string>();
  const tasks = new Set<string>();
  editor.state.doc.descendants((node) => {
    if (node.type.name === "pageLink") pages.add(node.attrs.id);
    if (node.type.name === "taskMention") tasks.add(node.attrs.id);
  });
  return { pageIds: [...pages], taskIds: [...tasks] };
}

/** An inline reference to another record, saved as its id plus the label it had when inserted. */
function reference(name: string, kind: "pages" | "tasks", dataAttr: string, prefix: string) {
  return Node.create({
    name,
    group: "inline",
    inline: true,
    atom: true,
    selectable: true,
    addAttributes() {
      return { id: { default: null }, label: { default: "" } };
    },
    parseHTML: () => [{ tag: `span[${dataAttr}]` }],
    renderHTML: ({ node }) => [
      "span",
      { [dataAttr]: node.attrs.id, class: "wiki-ref", role: "link" },
      `${prefix}${node.attrs.label || "Untitled"}`,
    ],
    renderText: ({ node }) => `${prefix}${node.attrs.label}`,
    addNodeView() {
      return ({ node, editor }) => {
        let current = node;
        const dom = document.createElement("span");
        dom.className = "wiki-ref";
        dom.setAttribute("role", "link");
        const storage = editor.storage.references;
        const paint = () => {
          const found = storage[kind].get(current.attrs.id);
          dom.setAttribute(dataAttr, current.attrs.id);
          dom.textContent = `${prefix}${found?.title || current.attrs.label || "Untitled"}`;
          if (found?.deleted) {
            dom.setAttribute("data-gone", "");
            dom.title = "In the trash";
          } else {
            dom.removeAttribute("data-gone");
            dom.removeAttribute("title");
          }
        };
        storage.paint.add(paint);
        paint();
        return {
          dom,
          update(updated) {
            if (updated.type !== current.type) return false;
            current = updated;
            paint();
            return true;
          },
          destroy() {
            storage.paint.delete(paint);
          },
        };
      };
    },
  });
}

export const TaskMention = reference("taskMention", "tasks", "data-task", "@");
export const PageLink = reference("pageLink", "pages", "data-page", "");
