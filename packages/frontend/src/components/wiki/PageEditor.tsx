import { useCallback, useEffect, useRef, useState, type RefObject } from "react";
import { useMutation, useQueryClient } from "@tanstack/react-query";
import { liveSession } from "woodmouse-sdk";
import type { LiveSession, PageDetail } from "woodmouse-sdk";
import { listPagesQueryKey, updatePageMutation } from "woodmouse-sdk/react-query";

import { userLabel } from "../../lib/user";
import { LiveBody } from "./LiveBody";
import { ManualBody } from "./ManualBody";
import { usePresence } from "./usePresence";

const TITLE_DELAY_MS = 800;

type Mode = { kind: "loading" } | { kind: "live"; session: LiveSession } | { kind: "manual" };

/**
 * The title and content of one page. Content is edited live with everyone else on the page; when the
 * live connection is not available it falls back to manual saving (see `ManualBody`).
 */
export function PageEditor({
  page,
  workspaceId,
  editable,
  saveRef,
  onReload,
}: {
  page: PageDetail;
  workspaceId: string;
  editable: boolean;
  /** Filled with a function that saves pending edits now (a no-op while editing live). */
  saveRef?: RefObject<(() => Promise<void>) | null>;
  /** Load the page again: the live document was rebuilt (a restore, or a save without live sync). */
  onReload: () => void;
}) {
  const queryClient = useQueryClient();
  const [mode, setMode] = useState<Mode>({ kind: "loading" });
  const [title, setTitle] = useState(page.title);
  const titleInput = useRef<HTMLInputElement>(null);

  // Ask the server to set up live editing; if that fails, edit manually.
  useEffect(() => {
    let cancelled = false;
    void liveSession({ path: { id: page.id } }).then(({ data }) => {
      if (!cancelled) setMode(data ? { kind: "live", session: data } : { kind: "manual" });
    });
    return () => {
      cancelled = true;
    };
  }, [page.id]);

  const manual = mode.kind === "manual";
  const others = usePresence(page.id, editable && mode.kind !== "loading", manual);
  const unsyncedEditors = others.filter((o) => o.fallback);

  const unavailable = useCallback(() => setMode({ kind: "manual" }), []);

  useEffect(() => {
    if (!saveRef || mode.kind === "manual") return;
    // Live editing saves by itself on the server.
    saveRef.current = () => Promise.resolve();
    return () => {
      saveRef.current = null;
    };
  }, [saveRef, mode.kind]);

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

  return (
    <div className="flex flex-col gap-4">
      {unsyncedEditors.length > 0 ? (
        <p role="status" className="border border-destructive bg-card px-3 py-2 text-sm">
          <strong>{unsyncedEditors.map((o) => userLabel(o.user)).join(", ")}</strong>{" "}
          {unsyncedEditors.length === 1 ? "is" : "are"} editing without live sync. Saving there
          replaces the live version, so recent changes here can be lost.
        </p>
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
              (document.querySelector(".wiki-editor") as HTMLElement | null)?.focus();
            }
          }}
          className="w-full bg-transparent font-display text-3xl font-bold leading-tight outline-none placeholder:text-muted-foreground"
        />
      </div>

      {mode.kind === "loading" ? <p className="text-muted-foreground">Connecting…</p> : null}
      {mode.kind === "live" ? (
        <LiveBody
          pageId={page.id}
          workspaceId={workspaceId}
          editable={editable}
          session={mode.session}
          onReload={onReload}
          onUnavailable={unavailable}
        />
      ) : null}
      {mode.kind === "manual" ? (
        <ManualBody page={page} workspaceId={workspaceId} editable={editable} saveRef={saveRef} />
      ) : null}
    </div>
  );
}
