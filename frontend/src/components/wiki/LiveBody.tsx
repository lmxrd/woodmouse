import { useEffect, useMemo, useState } from "react";
import Collaboration from "@tiptap/extension-collaboration";
import CollaborationCaret from "@tiptap/extension-collaboration-caret";
import { EditorContent, useEditor } from "@tiptap/react";
import { useQuery, useQueryClient } from "@tanstack/react-query";
import { WebsocketProvider } from "y-websocket";
import * as Y from "yjs";
import type { LiveSession } from "woodmouse-client";
import { meOptions } from "woodmouse-client/react-query";

import { colorFor } from "../../lib/liveColors";
import { userLabel } from "../../lib/user";
import { Collaborators } from "./Collaborators";
import { editorExtensions } from "./editor/extensions";
import { FormatToolbar } from "./editor/FormatToolbar";
import { useEditorClicks } from "./useEditorClicks";
import { useFileLibraryPicker } from "./useFileLibraryPicker";
import { useReferences } from "./useReferences";

/** How long to wait for the first connection before offering manual saving instead. */
const CONNECT_TIMEOUT_MS = 8000;

/** Close codes from the server (backend `live::close`). */
const RESET = 4001;
const DELETED = 4404;
const FORBIDDEN = 4403;

const apiBase = () => import.meta.env.VITE_API_URL ?? window.location.origin;

type Status = "connecting" | "connected" | "disconnected";

/**
 * The page content, edited live with everyone else who has it open. The server keeps the shared
 * document and saves it as the page; the browser only needs the connection.
 */
export function LiveBody({
  pageId,
  workspaceId,
  editable,
  session,
  onReload,
  onUnavailable,
}: {
  pageId: string;
  workspaceId: string;
  editable: boolean;
  session: LiveSession;
  /** The server rebuilt the document (a restore, or a save without live sync): load the page again. */
  onReload: () => void;
  /** The connection could not be made: fall back to manual saving. */
  onUnavailable: () => void;
}) {
  const [live, setLive] = useState<{ ydoc: Y.Doc; provider: WebsocketProvider } | null>(null);
  const [status, setStatus] = useState<Status>("connecting");
  const [ended, setEnded] = useState<number | null>(null);
  const { data: me } = useQuery(meOptions());

  useEffect(() => {
    const ydoc = new Y.Doc();
    const url = apiBase().replace(/^http/, "ws");
    const provider = new WebsocketProvider(`${url}/api/pages`, `${pageId}/live`, ydoc, {
      params: { epoch: session.epoch },
      disableBc: true,
      // Not retrying once the server has said the document is gone or rebuilt.
      shouldReconnect: (event) => ![RESET, DELETED, FORBIDDEN].includes(event.code),
    });
    let everConnected = false;
    const onStatus = ({ status }: { status: Status }) => {
      if (status === "connected") everConnected = true;
      setStatus(status);
    };
    const onClose = (event: CloseEvent | null) => {
      if (event?.code === RESET) onReload();
      else if (event && [DELETED, FORBIDDEN].includes(event.code)) setEnded(event.code);
    };
    provider.on("status", onStatus);
    provider.on("connection-close", onClose);
    const timer = setTimeout(() => {
      if (!everConnected) onUnavailable();
    }, CONNECT_TIMEOUT_MS);
    // The shared document and its connection are created here and handed to the editor once they exist.
    // oxlint-disable-next-line react/set-state-in-effect
    setLive({ ydoc, provider });
    return () => {
      clearTimeout(timer);
      provider.off("status", onStatus);
      provider.off("connection-close", onClose);
      provider.destroy();
      ydoc.destroy();
      setLive(null);
    };
  }, [pageId, session.epoch, onReload, onUnavailable]);

  if (ended) {
    return (
      <p role="alert" className="border border-destructive bg-card p-3 text-sm">
        {ended === DELETED ? "This page was deleted." : "You no longer have access to this page."}
      </p>
    );
  }
  return (
    <div className="flex flex-col gap-4">
      <div className="flex min-h-6 items-center justify-between gap-2">
        {live ? <Collaborators provider={live.provider} /> : <span />}
        <p
          role="status"
          className={`label-mono ${status === "connected" ? "text-muted-foreground" : "text-destructive"}`}
        >
          {status === "connected"
            ? editable
              ? "Live · saved automatically"
              : "Live"
            : status === "connecting"
              ? "Connecting…"
              : "Reconnecting… your edits are kept"}
        </p>
      </div>
      {live && me ? (
        <LiveEditor
          key={live.provider.roomname}
          ydoc={live.ydoc}
          provider={live.provider}
          editable={editable && session.can_edit}
          workspaceId={workspaceId}
        />
      ) : null}
    </div>
  );
}

function LiveEditor({
  ydoc,
  provider,
  editable,
  workspaceId,
}: {
  ydoc: Y.Doc;
  provider: WebsocketProvider;
  editable: boolean;
  workspaceId: string;
}) {
  const queryClient = useQueryClient();
  const { data: me } = useQuery(meOptions());
  const name = me ? userLabel(me) : "Someone";
  const color = colorFor(me?.id ?? "");

  const extensions = useMemo(
    () => [
      ...editorExtensions({ workspaceId, queryClient, live: true }),
      Collaboration.configure({ document: ydoc, field: "default" }),
      CollaborationCaret.configure({ provider, user: { name, color } }),
    ],
    [workspaceId, queryClient, ydoc, provider, name, color],
  );
  const editor = useEditor(
    {
      extensions,
      editable,
      editorProps: { attributes: { class: "wiki-editor", "aria-label": "Page content" } },
    },
    [extensions, editable],
  );
  useReferences(editor, workspaceId);
  useEditorClicks(editor, workspaceId);
  const libraryPicker = useFileLibraryPicker(editor, workspaceId);

  return (
    <>
      {editor ? <FormatToolbar editor={editor} /> : null}
      <EditorContent editor={editor} />
      {libraryPicker}
    </>
  );
}
