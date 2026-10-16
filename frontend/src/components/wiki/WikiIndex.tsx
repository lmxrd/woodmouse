import { useQuery } from "@tanstack/react-query";
import { Link } from "@tanstack/react-router";
import { listPagesOptions } from "woodmouse-client/react-query";

import { wikiExportUrl } from "../../lib/api";
import { useFormatters } from "../../lib/useFormatters";
import { useWorkspaceRole } from "../../lib/useWorkspaceRole";
import { Button } from "../ui/button";
import { PageTree } from "./PageTree";
import { usePageActions } from "./usePageActions";

/** The wiki overview: the whole page tree, with recently edited pages on top. */
export function WikiIndex({ workspaceId }: { workspaceId: string }) {
  const { data: pages = [], isPending } = useQuery(listPagesOptions({ path: { id: workspaceId } }));
  const { canWrite } = useWorkspaceRole(workspaceId);
  const { createPage } = usePageActions(workspaceId);
  const { when } = useFormatters();
  const recent = [...pages].sort((a, b) => b.updated_at - a.updated_at).slice(0, 5);

  return (
    <div className="mx-auto flex max-w-3xl flex-col gap-8 p-4 sm:p-8">
      <header className="flex items-center justify-between gap-3">
        <h1 className="font-display text-2xl font-bold">Wiki</h1>
        <div className="flex gap-2">
          {pages.length > 0 ? (
            <Button
              variant="outline"
              onClick={() => window.location.assign(wikiExportUrl(workspaceId))}
            >
              Export all
            </Button>
          ) : null}
          {canWrite ? <Button onClick={() => createPage()}>New page</Button> : null}
        </div>
      </header>

      {!isPending && pages.length === 0 ? (
        <p className="border border-dashed border-border px-4 py-6 text-muted-foreground">
          No pages yet.{canWrite ? " Create the first one to get started." : ""}
        </p>
      ) : null}

      {recent.length > 0 ? (
        <section className="flex flex-col gap-2">
          <h2 className="label-mono text-muted-foreground">Recently edited</h2>
          <ul className="grid gap-2 sm:grid-cols-2">
            {recent.map((page) => (
              <li key={page.id}>
                <Link
                  to="/w/$workspaceId/wiki/$pageId"
                  params={{ workspaceId, pageId: page.id }}
                  className="flex flex-col border border-border bg-card px-3 py-2 hover:border-foreground"
                >
                  <span className="truncate font-medium">{page.title}</span>
                  <span className="label-mono text-muted-foreground">{when(page.updated_at)}</span>
                </Link>
              </li>
            ))}
          </ul>
        </section>
      ) : null}

      {pages.length > 0 ? (
        <section className="flex flex-col gap-2">
          <h2 className="label-mono text-muted-foreground">All pages</h2>
          <PageTree pages={pages} workspaceId={workspaceId} canWrite={canWrite} openAll />
        </section>
      ) : null}
    </div>
  );
}
