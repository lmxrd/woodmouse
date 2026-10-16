import { useCallback, useEffect, useMemo, useRef, useState, type RefObject } from "react";
import { EditorContent, useEditor } from "@tiptap/react";
import { useBlocker } from "@tanstack/react-router";
import { useQueryClient } from "@tanstack/react-query";
import { getPage, saveBlocks } from "woodmouse-client";
import type { PageDetail } from "woodmouse-client";

import { fromBlocks, toBlocks } from "../../lib/editorBlocks";
import { userLabel } from "../../lib/user";
import { Button } from "../ui/button";
import { editorExtensions } from "./editor/extensions";
import { FormatToolbar } from "./editor/FormatToolbar";
import { useEditorClicks } from "./useEditorClicks";
import { useReferences } from "./useReferences";

type Status = "saved" | "unsaved" | "saving" | "error";

/**
 * The page content without live sync: autosave is off and changes are saved with the Save button.
 * Used when the live connection is not available. Each save says which version it started from, so if
 * someone else saved in the meantime the server refuses it and the person chooses between keeping their
 * text and loading the other version. A save replaces the live document, so people editing live reload.
 */
export function ManualBody({
  page,
  workspaceId,
  editable,
  saveRef,
}: {
  page: PageDetail;
  workspaceId: string;
  editable: boolean;
  saveRef?: RefObject<(() => Promise<void>) | null>;
}) {
  const queryClient = useQueryClient();
  const [status, setStatus] = useState<Status>("saved");
  const [conflict, setConflict] = useState<PageDetail | "unknown" | null>(null);
  const version = useRef(page.version);
  const dirty = useRef(false);

  const extensions = useMemo(
    () => editorExtensions({ workspaceId, queryClient }),
    [workspaceId, queryClient],
  );
  const editor = useEditor({
    extensions,
    content: fromBlocks(page.blocks),
    editable,
    editorProps: { attributes: { class: "wiki-editor", "aria-label": "Page content" } },
    onUpdate: ({ transaction }) => {
      if (!transaction.docChanged || !editable) return;
      dirty.current = true;
      setStatus("unsaved");
    },
  });
  useReferences(editor, workspaceId);
  useEditorClicks(editor, workspaceId);

  const save = useCallback(
    async (force = false) => {
      if (!editor || (!dirty.current && !force)) return;
      setStatus("saving");
      dirty.current = false;
      const { data, response } = await saveBlocks({
        path: { id: page.id },
        body: { blocks: toBlocks(editor.getJSON()), base_version: version.current, force },
      });
      if (data) {
        version.current = data.version;
        setConflict(null);
        setStatus(dirty.current ? "unsaved" : "saved");
        return;
      }
      dirty.current = true;
      if (response?.status === 409) {
        setStatus("unsaved");
        const theirs = await getPage({ path: { id: page.id } });
        setConflict(theirs.data ?? "unknown");
      } else {
        setStatus("error");
      }
    },
    [editor, page.id],
  );

  useEffect(() => {
    if (!saveRef) return;
    saveRef.current = () => save();
    return () => {
      saveRef.current = null;
    };
  }, [saveRef, save]);

  // Ctrl or Cmd + S saves.
  useEffect(() => {
    if (!editable) return;
    const onKey = (event: KeyboardEvent) => {
      if ((event.metaKey || event.ctrlKey) && event.key.toLowerCase() === "s") {
        event.preventDefault();
        void save();
      }
    };
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  }, [editable, save]);

  // Nothing saves by itself here, so leaving with unsaved changes asks first.
  useBlocker({
    shouldBlockFn: () =>
      dirty.current && !window.confirm("You have unsaved changes. Leave without saving?"),
    enableBeforeUnload: () => dirty.current,
  });

  const loadTheirs = () => {
    if (conflict && conflict !== "unknown") {
      editor?.commands.setContent(fromBlocks(conflict.blocks), { emitUpdate: false });
      version.current = conflict.version;
    }
    dirty.current = false;
    setConflict(null);
    setStatus("saved");
  };

  return (
    <div className="flex flex-col gap-4">
      {editable ? (
        <div role="alert" className="border border-border bg-card px-3 py-2 text-sm">
          <strong>Live editing is not available.</strong> This page is in manual mode: nothing is
          saved until you press Save, and saving replaces the live version for everyone who has the
          page open.
        </div>
      ) : null}
      {conflict ? (
        <div
          role="alert"
          className="flex flex-col gap-2 border border-destructive bg-card p-3 text-sm"
        >
          <p>
            {conflict !== "unknown" && conflict.updated_by
              ? `${userLabel(conflict.updated_by)} changed this page while you were editing.`
              : "This page was changed while you were editing."}{" "}
            Your latest edits are not saved yet.
          </p>
          <div className="flex flex-wrap gap-2">
            <Button size="sm" onClick={() => void save(true)}>
              Keep my version
            </Button>
            {conflict !== "unknown" ? (
              <Button size="sm" variant="outline" onClick={loadTheirs}>
                Load their version
              </Button>
            ) : null}
          </div>
        </div>
      ) : null}
      {editable ? (
        <div className="flex items-center justify-end gap-3">
          <span
            role="status"
            className={`label-mono ${status === "error" ? "text-destructive" : "text-muted-foreground"}`}
          >
            {status === "saved"
              ? "Saved"
              : status === "saving"
                ? "Saving…"
                : status === "error"
                  ? "Couldn't save"
                  : "Unsaved changes"}
          </span>
          <Button
            size="sm"
            disabled={status === "saved" || status === "saving"}
            onClick={() => void save()}
          >
            Save
          </Button>
        </div>
      ) : null}
      {editor ? <FormatToolbar editor={editor} /> : null}
      <EditorContent editor={editor} />
    </div>
  );
}
