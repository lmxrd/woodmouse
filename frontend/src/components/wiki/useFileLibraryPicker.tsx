import { useEffect, useState, type ReactNode } from "react";
import { useQuery } from "@tanstack/react-query";
import type { Editor } from "@tiptap/core";
import { listFilesOptions } from "woodmouse-client/react-query";

import { formatBytes } from "../../lib/upload";
import { ResponsiveDialog } from "../common/ResponsiveDialog";
import { insertFileBlock } from "./editor/files";

/** The editor event the "File from library" slash item raises to open the picker. */
export const PICK_FILE_EVENT = "wiki:pick-file";

/**
 * Lets someone insert an existing library file into the page. The slash menu lives inside the editor, so
 * it asks for the picker with a DOM event; render `dialog` next to the editor.
 */
export function useFileLibraryPicker(editor: Editor | null, workspaceId: string): ReactNode {
  const [open, setOpen] = useState(false);
  const { data: files } = useQuery({
    ...listFilesOptions({ path: { id: workspaceId } }),
    enabled: open,
  });

  useEffect(() => {
    if (!editor) return;
    const root = editor.view.dom;
    const show = () => setOpen(true);
    root.addEventListener(PICK_FILE_EVENT, show);
    return () => root.removeEventListener(PICK_FILE_EVENT, show);
  }, [editor]);

  return (
    <ResponsiveDialog open={open} onOpenChange={setOpen} title="Insert a file">
      <div className="flex flex-col gap-3 p-5">
        <h2 className="font-display text-xl font-bold">Insert a file</h2>
        <ul className="max-h-80 overflow-y-auto">
          {files?.length === 0 ? (
            <li className="text-sm text-muted-foreground">The library is empty.</li>
          ) : null}
          {files?.map((file) => (
            <li key={file.id}>
              <button
                type="button"
                className="flex w-full items-center justify-between gap-3 px-2 py-1.5 text-left text-sm hover:bg-accent"
                onClick={() => {
                  if (editor) insertFileBlock(editor, file);
                  setOpen(false);
                }}
              >
                <span className="truncate">{file.name}</span>
                <span className="label-mono shrink-0 text-muted-foreground">
                  {formatBytes(file.size)}
                </span>
              </button>
            </li>
          ))}
        </ul>
      </div>
    </ResponsiveDialog>
  );
}
