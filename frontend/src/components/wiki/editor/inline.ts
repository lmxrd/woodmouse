import { Node, mergeAttributes } from "@tiptap/core";

/** An inline reference to another record, stored as its id plus the label it had when inserted. */
function reference(name: string, dataAttr: string, prefix: string) {
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
    renderHTML: ({ node, HTMLAttributes }) => [
      "span",
      mergeAttributes(HTMLAttributes, {
        [dataAttr]: node.attrs.id,
        class: "wiki-ref",
        role: "link",
      }),
      `${prefix}${node.attrs.label || "Untitled"}`,
    ],
    renderText: ({ node }) => `${prefix}${node.attrs.label}`,
  });
}

export const TaskMention = reference("taskMention", "data-task", "@");
export const PageLink = reference("pageLink", "data-page", "");
