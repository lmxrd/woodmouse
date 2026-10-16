import { useState } from "react";
import type { Editor } from "@tiptap/core";
import { useEditorState } from "@tiptap/react";
import { BubbleMenu } from "@tiptap/react/menus";

import { cn } from "../../../lib/utils";
import { Button } from "../../ui/button";
import { Input } from "../../ui/input";

/** "example.com" is almost certainly a website; anything else (paths, mailto:, app:// ...) is kept as typed. */
export function normalizeUrl(input: string): string {
  const url = input.trim();
  if (/^[a-z][a-z0-9+.-]*:/i.test(url) || /^[/#.?]/.test(url)) return url;
  return /^[^\s/]+\.[^\s/]{2,}(\/\S*)?$/.test(url) ? `https://${url}` : url;
}

const MARKS = [
  { name: "bold", label: "Bold", text: "B", className: "font-bold" },
  { name: "italic", label: "Italic", text: "I", className: "italic" },
  { name: "strike", label: "Strikethrough", text: "S", className: "line-through" },
  { name: "code", label: "Code", text: "</>", className: "font-mono text-xs" },
] as const;

/** Floating toolbar over selected text: marks and a link editor. */
export function FormatToolbar({ editor }: { editor: Editor }) {
  const [editingLink, setEditingLink] = useState(false);
  const [href, setHref] = useState("");
  const state = useEditorState({
    editor,
    selector: ({ editor: e }) => ({
      bold: e.isActive("bold"),
      italic: e.isActive("italic"),
      strike: e.isActive("strike"),
      code: e.isActive("code"),
      link: e.isActive("link"),
      href: (e.getAttributes("link").href as string | undefined) ?? "",
    }),
  });

  const applyLink = () => {
    const url = normalizeUrl(href);
    const chain = editor.chain().focus().extendMarkRange("link");
    if (url) chain.setLink({ href: url }).run();
    else chain.unsetLink().run();
    setEditingLink(false);
  };

  return (
    <BubbleMenu
      editor={editor}
      options={{ placement: "top" }}
      shouldShow={({ editor: e, state: s }) =>
        e.isEditable && !s.selection.empty && !e.isActive("codeBlock") && !e.isActive("divider")
      }
    >
      <div
        role="toolbar"
        aria-label="Text formatting"
        className="flex items-center gap-0.5 border border-foreground bg-popover p-0.5 shadow-md"
        onKeyDown={(e) => {
          if (e.key === "Escape") {
            setEditingLink(false);
            editor.commands.focus();
          }
        }}
      >
        {editingLink ? (
          <form
            className="flex items-center gap-1"
            onSubmit={(e) => {
              e.preventDefault();
              applyLink();
            }}
          >
            <Input
              autoFocus
              aria-label="Link address"
              value={href}
              onChange={(e) => setHref(e.target.value)}
              placeholder="Any URL, empty removes the link"
              className="h-7 w-56 text-xs"
            />
            <Button type="submit" size="xs">
              Apply
            </Button>
          </form>
        ) : (
          <>
            {MARKS.map((mark) => (
              <Button
                key={mark.name}
                type="button"
                variant="ghost"
                size="icon-xs"
                aria-label={mark.label}
                aria-pressed={state[mark.name]}
                className={cn(mark.className, state[mark.name] && "bg-accent")}
                onMouseDown={(e) => e.preventDefault()}
                onClick={() => editor.chain().focus().toggleMark(mark.name).run()}
              >
                {mark.text}
              </Button>
            ))}
            <Button
              type="button"
              variant="ghost"
              size="xs"
              aria-pressed={state.link}
              className={cn("underline", state.link && "bg-accent")}
              onMouseDown={(e) => e.preventDefault()}
              onClick={() => {
                setHref(state.href);
                setEditingLink(true);
              }}
            >
              Link
            </Button>
          </>
        )}
      </div>
    </BubbleMenu>
  );
}
