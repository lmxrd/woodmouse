import { useImperativeHandle, useState, type ReactNode, type Ref } from "react";
import { Extension, type Editor, type Range } from "@tiptap/core";
import { PluginKey } from "@tiptap/pm/state";
import { ReactRenderer } from "@tiptap/react";
import Suggestion, { type SuggestionProps } from "@tiptap/suggestion";

import { cn } from "../../../lib/utils";

export interface ListHandle {
  onKeyDown: (event: KeyboardEvent) => boolean;
}

interface ListProps<T> extends SuggestionProps<T, T> {
  render: (item: T) => ReactNode;
  empty: string;
  ref?: Ref<ListHandle>;
}

/** The popup list: arrow keys move, Enter or Tab picks, clicks pick. */
function SuggestionList<T>({ items, command, render, empty, ref }: ListProps<T>) {
  // The highlighted row restarts at the top whenever the matches change.
  const [pick, setPick] = useState({ items, index: 0 });
  const selected = pick.items === items ? pick.index : 0;
  const setSelected = (next: number | ((i: number) => number)) =>
    setPick({ items, index: typeof next === "function" ? next(selected) : next });

  useImperativeHandle(ref, () => ({
    onKeyDown(event) {
      if (items.length === 0) return false;
      if (event.key === "ArrowDown") setSelected((i) => (i + 1) % items.length);
      else if (event.key === "ArrowUp") setSelected((i) => (i - 1 + items.length) % items.length);
      else if (event.key === "Enter" || event.key === "Tab") command(items[selected]);
      else return false;
      return true;
    },
  }));

  return (
    <div
      role="listbox"
      className="max-h-72 w-72 max-w-[calc(100vw-1rem)] overflow-y-auto border border-foreground bg-popover p-1 text-sm shadow-md"
    >
      {items.length === 0 ? (
        <p className="px-2 py-1.5 text-muted-foreground">{empty}</p>
      ) : (
        items.map((item, i) => (
          <button
            key={i}
            type="button"
            role="option"
            aria-selected={i === selected}
            // Keep the editor's focus and selection while picking.
            onMouseDown={(e) => e.preventDefault()}
            onClick={() => command(item)}
            onMouseEnter={() => setSelected(i)}
            className={cn(
              "flex w-full items-center gap-2 px-2 py-1.5 text-left",
              i === selected && "bg-accent",
            )}
          >
            {render(item)}
          </button>
        ))
      )}
    </div>
  );
}

/** Place the popup under the trigger text, or above it when there is no room below. */
function place(element: HTMLElement, rect: DOMRect | null | undefined) {
  if (!rect) return;
  const height = element.offsetHeight;
  const below = rect.bottom + 6;
  const top =
    below + height > window.innerHeight && rect.top > height ? rect.top - height - 6 : below;
  element.style.position = "fixed";
  element.style.zIndex = "60";
  element.style.top = `${Math.max(4, top)}px`;
  element.style.left = `${Math.max(4, Math.min(rect.left, window.innerWidth - 292))}px`;
}

interface SuggestOptions<T> {
  name: string;
  /** The text that opens the menu. */
  char: string;
  items: (query: string) => T[] | Promise<T[]>;
  /** Called with the picked item; `range` covers the trigger and the query. */
  onPick: (editor: Editor, range: Range, item: T) => void;
  render: (item: T) => ReactNode;
  empty: string;
  /** Allow spaces in the query (search-as-you-type for titles). */
  allowSpaces?: boolean;
}

/** An editor extension that shows a pick list after `char` is typed. */
export function suggestionExtension<T>(options: SuggestOptions<T>) {
  return Extension.create({
    name: options.name,
    addProseMirrorPlugins() {
      return [
        Suggestion<T, T>({
          editor: this.editor,
          pluginKey: new PluginKey(options.name),
          char: options.char,
          allowSpaces: options.allowSpaces,
          items: ({ query }) => options.items(query),
          command: ({ editor, range, props }) => options.onPick(editor, range, props),
          render: () => {
            let component: ReactRenderer<ListHandle, ListProps<T>> | null = null;
            const props = (p: SuggestionProps<T, T>) => ({
              ...p,
              render: options.render,
              empty: options.empty,
            });
            return {
              onStart(p) {
                component = new ReactRenderer(SuggestionList<T>, {
                  props: props(p),
                  editor: p.editor,
                });
                document.body.appendChild(component.element);
                place(component.element, p.clientRect?.());
              },
              onUpdate(p) {
                component?.updateProps(props(p));
                if (component) place(component.element, p.clientRect?.());
              },
              onKeyDown: ({ event }) => component?.ref?.onKeyDown(event) ?? false,
              onExit() {
                component?.element.remove();
                component?.destroy();
                component = null;
              },
            };
          },
        }),
      ];
    },
  });
}
