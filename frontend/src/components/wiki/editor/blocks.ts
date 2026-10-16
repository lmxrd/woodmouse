import {
  Extension,
  InputRule,
  Node,
  type Attributes,
  mergeAttributes,
  nodeInputRule,
  textblockTypeInputRule,
} from "@tiptap/core";
import type { Editor } from "@tiptap/core";
import type { NodeType } from "@tiptap/pm/model";
import { Plugin, PluginKey } from "@tiptap/pm/state";
import { Decoration, DecorationSet } from "@tiptap/pm/view";

/**
 * The block types the stored format has beyond paragraph, heading and code (which StarterKit
 * provides). Every block is a top-level node: lists are flat items with an `indent`, as stored.
 */

const MAX_INDENT = 8;
const LIST_NODES = ["bulletItem", "numberedItem", "todoItem"];
const TONES = ["info", "warning", "success", "danger"] as const;

/** Enter in an item continues it; Enter in an empty one outdents, then turns into a paragraph. */
function enter(editor: Editor, name: string) {
  const { state } = editor;
  const { $from, empty } = state.selection;
  const node = $from.parent;
  if (!empty || node.type.name !== name) return false;
  if (node.content.size === 0) {
    const indent = (node.attrs.indent as number | undefined) ?? 0;
    if (indent > 0) return editor.commands.updateAttributes(name, { indent: indent - 1 });
    return editor.commands.setNode("paragraph");
  }
  return editor.commands.command(({ tr, dispatch }) => {
    if (dispatch) {
      tr.split(tr.selection.from, 1, [
        { type: node.type, attrs: { ...node.attrs, checked: false } },
      ]);
      dispatch(tr.scrollIntoView());
    }
    return true;
  });
}

/** Backspace at the very start of an item removes the marker (outdent first). */
function backspace(editor: Editor, name: string) {
  const { $from, empty } = editor.state.selection;
  if (!empty || $from.parent.type.name !== name || $from.parentOffset !== 0) return false;
  const indent = (editor.getAttributes(name).indent as number | undefined) ?? 0;
  if (indent > 0) return editor.commands.updateAttributes(name, { indent: indent - 1 });
  return editor.commands.setNode("paragraph");
}

function textBlock(
  name: string,
  options: {
    attrs?: () => Attributes;
    rules?: (type: NodeType) => InputRule[];
    indentable?: boolean;
  },
) {
  return Node.create({
    name,
    group: "block",
    content: "inline*",
    defining: true,
    addAttributes: options.attrs,
    parseHTML: () => [{ tag: `div[data-block="${name}"]` }],
    renderHTML: ({ HTMLAttributes }) => [
      "div",
      mergeAttributes(HTMLAttributes, { "data-block": name }),
      0,
    ],
    addInputRules() {
      return options.rules?.(this.type) ?? [];
    },
    addKeyboardShortcuts() {
      const keys: Record<string, () => boolean> = {
        Enter: () => enter(this.editor, name),
        Backspace: () => backspace(this.editor, name),
      };
      if (options.indentable) {
        const shift = (by: number) => () => {
          if (this.editor.state.selection.$from.parent.type.name !== name) return false;
          const indent = (this.editor.getAttributes(name).indent as number | undefined) ?? 0;
          const next = Math.min(MAX_INDENT, Math.max(0, indent + by));
          this.editor.commands.updateAttributes(name, { indent: next });
          return true; // never let Tab leave the editor
        };
        keys.Tab = shift(1);
        keys["Shift-Tab"] = shift(-1);
      }
      return keys;
    },
  });
}

const indentAttr: Attributes = {
  indent: {
    default: 0,
    parseHTML: (el: HTMLElement) => Number(el.getAttribute("data-indent")) || 0,
    renderHTML: (attrs) => ({
      "data-indent": attrs.indent,
      style: `--indent:${attrs.indent}`,
    }),
  },
};

export const BulletItem = textBlock("bulletItem", {
  indentable: true,
  attrs: () => indentAttr,
  rules: (type) => [textblockTypeInputRule({ find: /^\s*[-+*]\s$/, type })],
});

export const NumberedItem = textBlock("numberedItem", {
  indentable: true,
  attrs: () => indentAttr,
  rules: (type) => [textblockTypeInputRule({ find: /^\s*\d+\.\s$/, type })],
});

export const Quote = textBlock("quote", {
  rules: (type) => [textblockTypeInputRule({ find: /^\s*>\s$/, type })],
});

/** A highlighted note; click its icon to cycle info, warning, success and danger. */
export const Callout = Node.create({
  name: "callout",
  group: "block",
  content: "inline*",
  defining: true,
  addAttributes() {
    return {
      tone: {
        default: "info",
        parseHTML: (el) => el.getAttribute("data-tone") ?? "info",
        renderHTML: (attrs) => ({ "data-tone": attrs.tone }),
      },
    };
  },
  parseHTML: () => [{ tag: 'div[data-block="callout"]' }],
  renderHTML: ({ HTMLAttributes }) => [
    "div",
    mergeAttributes(HTMLAttributes, { "data-block": "callout" }),
    0,
  ],
  addNodeView() {
    return ({ node, editor, getPos }) => {
      let current = node;
      const dom = document.createElement("div");
      dom.dataset.block = "callout";
      const icon = document.createElement("button");
      icon.type = "button";
      icon.contentEditable = "false";
      icon.className = "wiki-callout-icon";
      icon.setAttribute("aria-label", "Change callout style");
      const content = document.createElement("div");
      content.className = "wiki-callout-body";
      dom.append(icon, content);
      const paint = () => {
        dom.dataset.tone = current.attrs.tone;
        icon.textContent = { info: "i", warning: "!", success: "✓", danger: "×" }[
          current.attrs.tone as (typeof TONES)[number]
        ];
      };
      paint();
      icon.addEventListener("mousedown", (e) => e.preventDefault());
      icon.addEventListener("click", () => {
        const pos = getPos();
        if (!editor.isEditable || typeof pos !== "number") return;
        const next = TONES[(TONES.indexOf(current.attrs.tone) + 1) % TONES.length];
        editor.view.dispatch(editor.state.tr.setNodeMarkup(pos, undefined, { tone: next }));
      });
      return {
        dom,
        contentDOM: content,
        update(updated) {
          if (updated.type !== current.type) return false;
          current = updated;
          paint();
          return true;
        },
      };
    };
  },
  addKeyboardShortcuts() {
    return {
      Enter: () => enter(this.editor, "callout"),
      Backspace: () => backspace(this.editor, "callout"),
    };
  },
});

export const TodoItem = Node.create({
  name: "todoItem",
  group: "block",
  content: "inline*",
  defining: true,
  addAttributes() {
    return {
      ...indentAttr,
      checked: {
        default: false,
        parseHTML: (el) => el.getAttribute("data-checked") === "true",
        renderHTML: (attrs) => ({ "data-checked": String(!!attrs.checked) }),
      },
    };
  },
  parseHTML: () => [{ tag: 'div[data-block="todoItem"]' }],
  renderHTML: ({ HTMLAttributes }) => [
    "div",
    mergeAttributes(HTMLAttributes, { "data-block": "todoItem" }),
    0,
  ],
  addNodeView() {
    return ({ node, editor, getPos }) => {
      let current = node;
      const dom = document.createElement("div");
      dom.dataset.block = "todoItem";
      const box = document.createElement("input");
      box.type = "checkbox";
      box.contentEditable = "false";
      box.setAttribute("aria-label", "Done");
      const content = document.createElement("div");
      content.className = "wiki-todo-body";
      dom.append(box, content);
      const paint = () => {
        dom.dataset.checked = String(!!current.attrs.checked);
        dom.dataset.indent = String(current.attrs.indent);
        dom.style.setProperty("--indent", String(current.attrs.indent));
        box.checked = !!current.attrs.checked;
      };
      paint();
      box.addEventListener("change", () => {
        const pos = getPos();
        if (!editor.isEditable || typeof pos !== "number") {
          box.checked = !!current.attrs.checked;
          return;
        }
        editor.view.dispatch(
          editor.state.tr.setNodeMarkup(pos, undefined, { ...current.attrs, checked: box.checked }),
        );
      });
      return {
        dom,
        contentDOM: content,
        update(updated) {
          if (updated.type !== current.type) return false;
          current = updated;
          paint();
          return true;
        },
      };
    };
  },
  addInputRules() {
    return [
      textblockTypeInputRule({
        find: /^\s*\[([ xX]?)\]\s$/,
        type: this.type,
        getAttributes: (match) => ({ checked: match[1]?.toLowerCase() === "x" }),
      }),
    ];
  },
  addKeyboardShortcuts() {
    const shift = (by: number) => () => {
      if (this.editor.state.selection.$from.parent.type.name !== "todoItem") return false;
      const indent = (this.editor.getAttributes("todoItem").indent as number | undefined) ?? 0;
      this.editor.commands.updateAttributes("todoItem", {
        indent: Math.min(MAX_INDENT, Math.max(0, indent + by)),
      });
      return true;
    };
    return {
      Enter: () => enter(this.editor, "todoItem"),
      Backspace: () => backspace(this.editor, "todoItem"),
      Tab: shift(1),
      "Shift-Tab": shift(-1),
    };
  },
});

export const Divider = Node.create({
  name: "divider",
  group: "block",
  atom: true,
  selectable: true,
  parseHTML: () => [{ tag: "hr" }],
  renderHTML: () => ["hr"],
  addInputRules() {
    return [nodeInputRule({ find: /^(?:---|___\s|\*\*\*\s)$/, type: this.type })];
  },
});

const numberingKey = new PluginKey("numbering");

/** Numbered items show 1., 2., ... counting runs of consecutive items at the same indent. */
export const Numbering = Extension.create({
  name: "numbering",
  addProseMirrorPlugins() {
    return [
      new Plugin({
        key: numberingKey,
        props: {
          decorations(state) {
            const decorations: Decoration[] = [];
            let counters: number[] = [];
            state.doc.forEach((node, offset) => {
              const indent = (node.attrs.indent as number | undefined) ?? 0;
              if (node.type.name === "numberedItem") {
                counters = counters.slice(0, indent + 1);
                while (counters.length <= indent) counters.push(0);
                counters[indent] += 1;
                decorations.push(
                  Decoration.node(offset, offset + node.nodeSize, {
                    "data-num": `${counters[indent]}.`,
                  }),
                );
              } else if (LIST_NODES.includes(node.type.name)) {
                counters = counters.slice(0, indent);
              } else {
                counters = [];
              }
            });
            return DecorationSet.create(state.doc, decorations);
          },
        },
      }),
    ];
  },
});
