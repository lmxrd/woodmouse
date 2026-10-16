import { useEffect } from "react";
import type { Editor } from "@tiptap/core";
import { resolve } from "woodmouse-client";

import { referencedInEditor, setReferences, type RefInfo } from "./editor/inline";

const LOOKUP_DELAY_MS = 300;

/**
 * Keep the editor's mentions and links showing their targets' current titles (and which are in the
 * trash). Looks up whatever the document refers to when it opens and whenever the set of targets changes.
 */
export function useReferences(editor: Editor | null, workspaceId: string) {
  useEffect(() => {
    if (!editor) return;
    let timer: ReturnType<typeof setTimeout> | undefined;
    let known = "";
    let cancelled = false;

    const lookup = async () => {
      const { pageIds, taskIds } = referencedInEditor(editor);
      const key = [...pageIds.sort(), "|", ...taskIds.sort()].join(",");
      if (key === known) return;
      known = key;
      if (pageIds.length + taskIds.length === 0) return;
      const { data } = await resolve({
        path: { id: workspaceId },
        body: { page_ids: pageIds, task_ids: taskIds },
      });
      if (!data || cancelled || editor.isDestroyed) return;
      const entries = (list: typeof data.pages) =>
        new Map<string, RefInfo>(list.map((r) => [r.id, { title: r.title, deleted: r.deleted }]));
      setReferences(editor, { pages: entries(data.pages), tasks: entries(data.tasks) });
    };
    const schedule = () => {
      clearTimeout(timer);
      timer = setTimeout(() => void lookup(), LOOKUP_DELAY_MS);
    };

    schedule();
    editor.on("update", schedule);
    return () => {
      cancelled = true;
      clearTimeout(timer);
      editor.off("update", schedule);
    };
  }, [editor, workspaceId]);
}
