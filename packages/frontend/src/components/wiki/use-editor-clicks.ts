import { useEffect } from "react";
import type { Editor } from "@tiptap/core";
import { useNavigate } from "@tanstack/react-router";

/**
 * Clicking a mention or page link opens it (a plain click, even while editing). Links open on a click
 * while reading, or with Ctrl or Cmd while editing.
 */
export function useEditorClicks(editor: Editor | null, workspaceId: string) {
  const navigate = useNavigate();
  useEffect(() => {
    if (!editor) return;
    const root = editor.view.dom;
    const onClick = (event: MouseEvent) => {
      const target = event.target as HTMLElement;
      const ref = target.closest<HTMLElement>("[data-task],[data-page]");
      if (ref?.dataset.task) {
        void navigate({
          to: "/w/$workspaceId/t/$taskId",
          params: { workspaceId, taskId: ref.dataset.task },
        });
      } else if (ref?.dataset.page) {
        void navigate({
          to: "/w/$workspaceId/wiki/$pageId",
          params: { workspaceId, pageId: ref.dataset.page },
        });
      } else {
        const link = target.closest("a");
        if (link && (!editor.isEditable || event.metaKey || event.ctrlKey)) {
          window.open(link.href, "_blank", "noopener,noreferrer");
        }
      }
    };
    root.addEventListener("click", onClick);
    return () => root.removeEventListener("click", onClick);
  }, [editor, navigate, workspaceId]);
}
