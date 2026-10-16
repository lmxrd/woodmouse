import { useQuery } from "@tanstack/react-query";
import { Link } from "@tanstack/react-router";
import { getPageOptions } from "woodmouse-client/react-query";

import { useWorkspaceRole } from "../../lib/useWorkspaceRole";
import { PageEditor } from "./PageEditor";
import { PageMenu } from "./PageMenu";

/** One wiki page: breadcrumb, actions, and the editor (read-only for members who cannot write). */
export function WikiPage({ workspaceId, pageId }: { workspaceId: string; pageId: string }) {
  const { canWrite } = useWorkspaceRole(workspaceId);
  // Fetched fresh on every visit and never refetched underneath the editor.
  const { data: page, isError } = useQuery({
    ...getPageOptions({ path: { id: pageId } }),
    gcTime: 0,
    staleTime: Infinity,
    refetchOnWindowFocus: false,
    refetchOnReconnect: false,
  });

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
        {canWrite ? <PageMenu page={page} workspaceId={workspaceId} leaveOnDelete /> : null}
      </div>
      <PageEditor key={page.id} page={page} workspaceId={workspaceId} editable={canWrite} />
    </div>
  );
}
