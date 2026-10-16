import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import { EditorContent, useEditor } from "@tiptap/react";
import { useMutation, useQueryClient } from "@tanstack/react-query";
import { useNavigate } from "@tanstack/react-router";
import { getPage, pingPresence, saveBlocks } from "woodmouse-client";
import type { Editor as Editor_, PageDetail } from "woodmouse-client";
import { listPagesQueryKey, updatePageMutation } from "woodmouse-client/react-query";

import { fromBlocks, toBlocks } from "../../lib/editorBlocks";
import { userLabel } from "../../lib/user";
import { SaveStatus } from "../SaveStatus";
import { Button } from "../ui/button";
import { editorExtensions } from "./editor/extensions";
import { FormatToolbar } from "./editor/FormatToolbar";

const SAVE_DELAY_MS = 1500;
const TITLE_DELAY_MS = 800;
const PRESENCE_EVERY_MS = 10_000;

type Status = "idle" | "pending" | "success" | "error";

/**
 * The title and content of one page, saved automatically.
 *
 * The editor reads the page once, when it opens: later refetches never overwrite what is being typed.
 * Each save says which `version` it started from, so if someone else saved in the meantime the server
 * refuses it and the person chooses between keeping their text and loading the other version.
 */
export function PageEditor({
  page,
  workspaceId,
  editable,
}: {
  page: PageDetail;
  workspaceId: string;
  editable: boolean;
}) {
  const queryClient = useQueryClient();
  const navigate = useNavigate();
  const [status, setStatus] = useState<Status>("idle");
  const [conflict, setConflict] = useState<PageDetail | "unknown" | null>(null);
  const [others, setOthers] = useState<Editor_[]>([]);
  const [title, setTitle] = useState(page.title);
  const titleInput = useRef<HTMLInputElement>(null);

  const version = useRef(page.version);
  const dirty = useRef(false);
  const timer = useRef<ReturnType<typeof setTimeout>>(undefined);
  const queue = useRef<Promise<void>>(Promise.resolve());
  const blocked = useRef(false);
  const editorRef = useRef<ReturnType<typeof useEditor>>(null);

  const extensions = useMemo(
    () => editorExtensions({ workspaceId, queryClient }),
    [workspaceId, queryClient],
  );

  const doSave = useCallback(
    async (force: boolean) => {
      const editor = editorRef.current;
      if (!editor || (!dirty.current && !force) || (blocked.current && !force)) return;
      dirty.current = false;
      setStatus("pending");
      const { data, error, response } = await saveBlocks({
        path: { id: page.id },
        body: { blocks: toBlocks(editor.getJSON()), base_version: version.current, force },
      });
      if (data) {
        version.current = data.version;
        blocked.current = false;
        setConflict(null);
        setStatus(dirty.current ? "pending" : "success");
        return;
      }
      dirty.current = true;
      if (response?.status === 409) {
        blocked.current = true;
        setStatus("idle");
        const theirs = await getPage({ path: { id: page.id } });
        setConflict(theirs.data ?? "unknown");
      } else {
        console.error("saving the page failed", error);
        setStatus("error");
      }
    },
    [page.id],
  );
  // Saves run one at a time, so a slow request is never overtaken by the next one.
  const flush = useCallback(
    (force = false) => {
      clearTimeout(timer.current);
      queue.current = queue.current.then(() => doSave(force));
      return queue.current;
    },
    [doSave],
  );

  const editor = useEditor({
    extensions,
    content: fromBlocks(page.blocks),
    editable,
    editorProps: { attributes: { class: "wiki-editor", "aria-label": "Page content" } },
    onUpdate: ({ transaction }) => {
      if (!transaction.docChanged || !editable) return;
      dirty.current = true;
      if (!blocked.current) {
        setStatus("pending");
        clearTimeout(timer.current);
        timer.current = setTimeout(() => void flush(), SAVE_DELAY_MS);
      }
    },
  });
  useEffect(() => {
    editorRef.current = editor;
  }, [editor]);

  // Clicking a mention or page link opens it (plain clicks navigate, even while editing).
  useEffect(() => {
    if (!editor) return;
    const root = editor.view.dom;
    const onClick = (event: MouseEvent) => {
      const ref = (event.target as HTMLElement).closest<HTMLElement>("[data-task],[data-page]");
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
        const link = (event.target as HTMLElement).closest("a");
        if (link && (!editor.isEditable || event.metaKey || event.ctrlKey)) {
          window.open(link.href, "_blank", "noopener,noreferrer");
        }
      }
    };
    root.addEventListener("click", onClick);
    return () => root.removeEventListener("click", onClick);
  }, [editor, navigate, workspaceId]);

  // Save what is pending when leaving the page or the tab.
  useEffect(() => {
    const onHide = () => {
      if (document.visibilityState === "hidden") void flush();
    };
    const onUnload = (event: BeforeUnloadEvent) => {
      if (dirty.current) {
        void flush();
        event.preventDefault();
      }
    };
    document.addEventListener("visibilitychange", onHide);
    window.addEventListener("beforeunload", onUnload);
    return () => {
      document.removeEventListener("visibilitychange", onHide);
      window.removeEventListener("beforeunload", onUnload);
      void flush();
    };
  }, [flush]);

  // Tell the server this page is open for editing, and learn who else has it open.
  useEffect(() => {
    if (!editable) return;
    const ping = async () => {
      if (document.visibilityState !== "visible") return;
      const { data } = await pingPresence({ path: { id: page.id } });
      if (data) setOthers(data);
    };
    void ping();
    const id = setInterval(() => void ping(), PRESENCE_EVERY_MS);
    return () => clearInterval(id);
  }, [editable, page.id]);

  // New pages open with the title ready to type.
  useEffect(() => {
    if (editable && page.title === "Untitled" && page.blocks.length === 0) {
      titleInput.current?.select();
    }
  }, [editable, page.title, page.blocks.length]);

  const rename = useMutation({
    ...updatePageMutation(),
    onSuccess: () =>
      queryClient.invalidateQueries({
        queryKey: listPagesQueryKey({ path: { id: workspaceId } }),
      }),
  });
  const titleTimer = useRef<ReturnType<typeof setTimeout>>(undefined);
  const saveTitle = (value: string) => {
    clearTimeout(titleTimer.current);
    if (value.trim() !== page.title && (value.trim() || page.title !== "Untitled")) {
      rename.mutate({ path: { id: page.id }, body: { title: value } });
    }
  };

  const loadTheirs = () => {
    if (conflict && conflict !== "unknown") {
      editor?.commands.setContent(fromBlocks(conflict.blocks), { emitUpdate: false });
      version.current = conflict.version;
    }
    dirty.current = false;
    blocked.current = false;
    setConflict(null);
    setStatus("success");
  };

  const who = (users: Editor_[]) => users.map((o) => userLabel(o.user)).join(", ");

  return (
    <div className="flex flex-col gap-4">
      {others.length > 0 ? (
        <p role="status" className="border border-border bg-card px-3 py-2 text-sm">
          <strong>{who(others)}</strong> {others.length === 1 ? "is" : "are"} also editing this
          page. Saves from both of you can collide.
        </p>
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
            <Button size="sm" onClick={() => void flush(true)}>
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

      <div className="flex flex-col gap-1">
        <input
          ref={titleInput}
          aria-label="Page title"
          value={title}
          readOnly={!editable}
          placeholder="Untitled"
          maxLength={200}
          onChange={(e) => {
            setTitle(e.target.value);
            clearTimeout(titleTimer.current);
            const value = e.target.value;
            titleTimer.current = setTimeout(() => saveTitle(value), TITLE_DELAY_MS);
          }}
          onBlur={(e) => saveTitle(e.target.value)}
          onKeyDown={(e) => {
            if (e.key === "Enter") {
              e.preventDefault();
              editor?.commands.focus("start");
            }
          }}
          className="w-full bg-transparent font-display text-3xl font-bold leading-tight outline-none placeholder:text-muted-foreground"
        />
        <div className="flex items-center justify-between gap-2">
          <p className="label-mono text-muted-foreground">
            {page.updated_by ? `Edited by ${userLabel(page.updated_by)}` : "New page"}
          </p>
          {editable ? <SaveStatus status={status} /> : null}
        </div>
      </div>

      {editor ? <FormatToolbar editor={editor} /> : null}
      <EditorContent editor={editor} />
    </div>
  );
}
