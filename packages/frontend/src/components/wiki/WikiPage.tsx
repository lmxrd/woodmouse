import { useCallback, useRef, useState } from "react";
import { useQuery, useQueryClient } from "@tanstack/react-query";
import { Link } from "@tanstack/react-router";
import { getPageOptions, getPageQueryKey, pageBacklinksOptions } from "woodmouse-sdk/react-query";

import { useWorkspaceRole } from "../../lib/useWorkspaceRole";
import { Attachments } from "../task/Attachments";
import { Backlinks } from "./Backlinks";
import { PageEditor } from "./PageEditor";
import { PageHistory } from "./PageHistory";
import { PageMenu } from "./PageMenu";

/** One wiki page: breadcrumb, actions, and the editor (read-only for members who cannot write). */
export function WikiPage({ workspaceId, pageId }: { workspaceId: string; pageId: string }) {
  const { canWrite } = useWorkspaceRole(workspaceId);
  const queryClient = useQueryClient();
  const [historyOpen, setHistoryOpen] = useState(false);
  // Bumped after a restore so the editor opens again with the restored content.
  const [reloads, setReloads] = useState(0);
  const saveRef = useRef<(() => Promise<void>) | null>(null);
  // Fetched fresh on every visit and never refetched underneath the editor.
  const { data: page, isError } = useQuery({
    ...getPageOptions({ path: { id: pageId } }),
    gcTime: 0,
    staleTime: Infinity,
    refetchOnWindowFocus: false,
    refetchOnReconnect: false,
  });

  // The server rebuilt the live document (a restore, or a save without live sync): fetch the page again
  // and open the editor afresh.
  const reload = useCallback(() => {
    void queryClient
      .invalidateQueries({ queryKey: getPageQueryKey({ path: { id: pageId } }) })
      .then(() => setReloads((n) => n + 1));
  }, [queryClient, pageId]);
  const { data: backlinks = [] } = useQuery(pageBacklinksOptions({ path: { id: pageId } }));

  if (isError) {
    return (
      <div className="mx-auto max-w-3xl p-8">
        <p>This page does not exist, or was deleted.</p>
        <Link to="/w/$workspaceId/wiki" params={{ workspaceId }} className="underline">
          Back to the wiki
        </Link>
      </div>
    );
  }
  if (!page) return <div className="mx-auto max-w-3xl p-8 text-muted-foreground">Loading…</div>;

  return (
    <div className="mx-auto flex max-w-3xl flex-col gap-4 p-4 sm:p-8">
      <div className="flex items-center justify-between gap-2">
        <nav
          aria-label="Breadcrumb"
          className="label-mono flex min-w-0 flex-wrap gap-1 text-muted-foreground"
        >
          <Link
            to="/w/$workspaceId/wiki"
            params={{ workspaceId }}
            className="hover:text-foreground"
          >
            Wiki
          </Link>
          {page.ancestors.map((a) => (
            <span key={a.id} className="flex gap-1">
              <span aria-hidden>/</span>
              <Link
                to="/w/$workspaceId/wiki/$pageId"
                params={{ workspaceId, pageId: a.id }}
                className="max-w-40 truncate hover:text-foreground"
              >
                {a.title}
              </Link>
            </span>
          ))}
        </nav>
        <PageMenu
          page={page}
          workspaceId={workspaceId}
          leaveOnDelete
          canWrite={canWrite}
          onHistory={() => {
            // Save what was just typed so it shows up as the current version.
            void (saveRef.current?.() ?? Promise.resolve()).then(() => setHistoryOpen(true));
          }}
        />
      </div>
      <PageEditor
        key={`${page.id}:${reloads}`}
        page={page}
        workspaceId={workspaceId}
        editable={canWrite}
        saveRef={saveRef}
        onReload={reload}
      />
      <Attachments pageId={page.id} workspaceId={workspaceId} editable={canWrite} />
      <Backlinks workspaceId={workspaceId} backlinks={backlinks} />
      <PageHistory
        pageId={page.id}
        workspaceId={workspaceId}
        open={historyOpen}
        onOpenChange={setHistoryOpen}
        canRestore={canWrite}
        onRestored={(restored) => {
          queryClient.setQueryData(getPageQueryKey({ path: { id: page.id } }), restored);
          setReloads((n) => n + 1);
        }}
      />
    </div>
  );
}
