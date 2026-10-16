import { Extension, type Editor } from "@tiptap/core";
import { Plugin, PluginKey } from "@tiptap/pm/state";
import { toast } from "sonner";

import { uploadFile } from "../../../lib/upload";

/** Put a library file into the page as a block, at `pos` or after the current selection. */
export function insertFileBlock(editor: Editor, file: { id: string; name: string }, pos?: number) {
  const node = { type: "fileBlock", attrs: { fileId: file.id, label: file.name } };
  const chain = editor.chain().focus();
  (pos == null ? chain.insertContent(node) : chain.insertContentAt(pos, node)).run();
}

/**
 * Upload files into the workspace's library and show each in the page. Content already in the library is
 * not stored twice: the existing file is shown instead.
 */
export async function insertFiles(
  editor: Editor,
  workspaceId: string,
  files: File[],
  pos?: number,
) {
  for (const file of files) {
    const result = await uploadFile(workspaceId, file);
    if (result.status === "created") insertFileBlock(editor, result.file, pos);
    else if (result.status === "duplicate") {
      toast.info(`${file.name} is already in the library; using that file.`);
      insertFileBlock(editor, result.existing, pos);
    } else if (result.status === "too_large") {
      toast.error(`${file.name} is larger than the upload limit.`);
    } else {
      toast.error(`Couldn't upload ${file.name}. Try again.`);
    }
    pos = undefined; // the rest follow the first
  }
}

/** Ask the browser for files (a hidden input, so it works on phones too) and insert them. */
export function chooseAndInsertFiles(editor: Editor, workspaceId: string) {
  const input = document.createElement("input");
  input.type = "file";
  input.multiple = true;
  input.onchange = () => void insertFiles(editor, workspaceId, [...(input.files ?? [])]);
  input.click();
}

/** Dropping or pasting files into the page uploads them and shows them where they landed. */
export const FileDrop = Extension.create<{ workspaceId: string }>({
  name: "fileDrop",
  addOptions: () => ({ workspaceId: "" }),
  addProseMirrorPlugins() {
    const { editor } = this;
    const workspaceId = this.options.workspaceId;
    return [
      new Plugin({
        key: new PluginKey("fileDrop"),
        props: {
          handleDrop(view, event) {
            const files = [...(event.dataTransfer?.files ?? [])];
            if (files.length === 0 || !view.editable) return false;
            event.preventDefault();
            const at = view.posAtCoords({ left: event.clientX, top: event.clientY });
            void insertFiles(editor, workspaceId, files, at?.pos);
            return true;
          },
          handlePaste(view, event) {
            const files = [...(event.clipboardData?.files ?? [])];
            if (files.length === 0 || !view.editable) return false;
            event.preventDefault();
            void insertFiles(editor, workspaceId, files);
            return true;
          },
        },
      }),
    ];
  },
});
