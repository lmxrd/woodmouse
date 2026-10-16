import type { Editor, Extensions, Range } from "@tiptap/core";
import Placeholder from "@tiptap/extension-placeholder";
import StarterKit from "@tiptap/starter-kit";
import type { QueryClient } from "@tanstack/react-query";
import type { SearchHit } from "woodmouse-client";
import { searchOptions } from "woodmouse-client/react-query";

import { cn } from "../../../lib/utils";
import { BulletItem, Callout, Divider, NumberedItem, Numbering, Quote, TodoItem } from "./blocks";
import { PageLink, TaskMention } from "./inline";
import { suggestionExtension } from "./suggestion";

/** Browsers ignore whitespace and control characters inside a URL scheme, so compare without them. */
function stripInvisible(url: string) {
  return [...url].filter((c) => c.charCodeAt(0) > 32).join("");
}

interface SlashItem {
  title: string;
  hint: string;
  keywords: string;
  run: (editor: Editor, range: Range) => void;
}

const block = (run: (chain: ReturnType<Editor["chain"]>) => { run: () => boolean }) => {
  return (editor: Editor, range: Range) => {
    run(editor.chain().focus().deleteRange(range)).run();
  };
};

const SLASH_ITEMS: SlashItem[] = [
  {
    title: "Text",
    hint: "Plain paragraph",
    keywords: "paragraph plain",
    run: block((c) => c.setParagraph()),
  },
  {
    title: "Heading 1",
    hint: "Big section title",
    keywords: "h1 title",
    run: block((c) => c.setNode("heading", { level: 1 })),
  },
  {
    title: "Heading 2",
    hint: "Medium section title",
    keywords: "h2",
    run: block((c) => c.setNode("heading", { level: 2 })),
  },
  {
    title: "Heading 3",
    hint: "Small section title",
    keywords: "h3",
    run: block((c) => c.setNode("heading", { level: 3 })),
  },
  {
    title: "Bulleted list",
    hint: "A simple list",
    keywords: "bullet ul unordered",
    run: block((c) => c.setNode("bulletItem")),
  },
  {
    title: "Numbered list",
    hint: "An ordered list",
    keywords: "ol ordered",
    run: block((c) => c.setNode("numberedItem")),
  },
  {
    title: "To-do list",
    hint: "Checkboxes to tick off",
    keywords: "todo task check checkbox",
    run: block((c) => c.setNode("todoItem")),
  },
  {
    title: "Quote",
    hint: "Set a quote apart",
    keywords: "blockquote",
    run: block((c) => c.setNode("quote")),
  },
  {
    title: "Callout",
    hint: "Highlighted note",
    keywords: "info warning note",
    run: block((c) => c.setNode("callout")),
  },
  {
    title: "Code",
    hint: "Monospaced code block",
    keywords: "snippet pre",
    run: block((c) => c.setCodeBlock()),
  },
  {
    title: "Divider",
    hint: "A horizontal rule",
    keywords: "hr line rule",
    run: block((c) => c.insertContent({ type: "divider" })),
  },
  {
    title: "Mention a task",
    hint: "Link to a task with @",
    keywords: "at task mention",
    run: block((c) => c.insertContent("@")),
  },
  {
    title: "Link to a page",
    hint: "Link to a wiki page with [[",
    keywords: "page wiki reference",
    run: block((c) => c.insertContent("[[")),
  },
];

function hitLabel(hit: SearchHit) {
  return (
    <span className="flex min-w-0 flex-1 flex-col leading-tight">
      <span className={cn("truncate", hit.is_completed && "text-muted-foreground line-through")}>
        {hit.title}
      </span>
    </span>
  );
}

export interface EditorContext {
  workspaceId: string;
  queryClient: QueryClient;
  /** Only the block types and rendering, without the pickers (for showing content, not editing it). */
  readOnly?: boolean;
}

/** The extensions of the page editor: the stored block types, markdown shortcuts and the pickers. */
export function editorExtensions({
  workspaceId,
  queryClient,
  readOnly = false,
}: EditorContext): Extensions {
  const find = (q: string, kind: "page" | "task") =>
    queryClient.fetchQuery({
      ...searchOptions({ path: { id: workspaceId }, query: { q, kind, limit: 5 } }),
      staleTime: 3000,
    });

  return [
    StarterKit.configure({
      heading: { levels: [1, 2, 3] },
      blockquote: false,
      bulletList: false,
      orderedList: false,
      listItem: false,
      listKeymap: false,
      horizontalRule: false,
      underline: false,
      link: {
        openOnClick: false,
        autolink: true,
        defaultProtocol: "https",
        // Any URL format is fine, except the schemes that run code.
        isAllowedUri: (url) => !/^(javascript|data|vbscript):/i.test(stripInvisible(url)),
      },
    }),
    Placeholder.configure({
      placeholder: ({ node }) =>
        node.type.name === "paragraph"
          ? "Type / for blocks, @ to mention a task, [[ to link a page"
          : "",
      showOnlyCurrent: true,
    }),
    BulletItem,
    NumberedItem,
    TodoItem,
    Quote,
    Callout,
    Divider,
    Numbering,
    TaskMention,
    PageLink,
    ...(readOnly ? [] : pickers()),
  ];

  function pickers(): Extensions {
    return [
      suggestionExtension<SlashItem>({
        name: "slashMenu",
        char: "/",
        empty: "No matching block",
        items: (query) => {
          const q = query.toLowerCase();
          return SLASH_ITEMS.filter((i) => `${i.title} ${i.keywords}`.toLowerCase().includes(q));
        },
        onPick: (editor, range, item) => item.run(editor, range),
        render: (item) => (
          <span className="flex min-w-0 flex-col leading-tight">
            <span>{item.title}</span>
            <span className="text-xs text-muted-foreground">{item.hint}</span>
          </span>
        ),
      }),
      suggestionExtension<SearchHit>({
        name: "taskPicker",
        char: "@",
        allowSpaces: true,
        empty: "No matching task",
        items: (query) => find(query, "task"),
        onPick: (editor, range, hit) =>
          editor
            .chain()
            .focus()
            .insertContentAt(range, [
              { type: "taskMention", attrs: { id: hit.id, label: hit.title, shown: hit.title } },
              { type: "text", text: " " },
            ])
            .run(),
        render: hitLabel,
      }),
      suggestionExtension<SearchHit>({
        name: "pagePicker",
        char: "[[",
        allowSpaces: true,
        empty: "No matching page",
        items: (query) => find(query, "page"),
        onPick: (editor, range, hit) =>
          editor
            .chain()
            .focus()
            .insertContentAt(range, [
              { type: "pageLink", attrs: { id: hit.id, label: hit.title, shown: hit.title } },
              { type: "text", text: " " },
            ])
            .run(),
        render: hitLabel,
      }),
    ];
  }
}
