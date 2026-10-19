import { useEffect, useMemo } from "react";
import { EditorContent, useEditor } from "@tiptap/react";
import { useQueryClient } from "@tanstack/react-query";
import type { BlockData } from "woodmouse-client";

import { fromBlocks } from "../../lib/editorBlocks";
import { editorExtensions } from "./editor/extensions";

/** Page content rendered exactly as in the editor, but not editable (used for earlier versions). */
export function BlockPreview({
  blocks,
  workspaceId,
}: {
  blocks: BlockData[];
  workspaceId: string;
}) {
  const queryClient = useQueryClient();
  const extensions = useMemo(
    () => editorExtensions({ workspaceId, queryClient, readOnly: true }),
    [workspaceId, queryClient],
  );
  const editor = useEditor({
    extensions,
    content: fromBlocks(blocks),
    editable: false,
    editorProps: { attributes: { class: "wiki-editor", "aria-label": "Version content" } },
  });
  useEffect(() => {
    editor?.commands.setContent(fromBlocks(blocks), { emitUpdate: false });
  }, [editor, blocks]);
  return <EditorContent editor={editor} />;
}
