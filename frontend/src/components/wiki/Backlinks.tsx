import { Link } from "@tanstack/react-router";
import type { Backlink } from "woodmouse-client";

/** "Referenced by": the pages whose content links to or mentions this. Renders nothing when empty. */
export function Backlinks({
  workspaceId,
  backlinks,
  heading = "Referenced by",
}: {
  workspaceId: string;
  backlinks: Backlink[];
  heading?: string;
}) {
  if (backlinks.length === 0) return null;
  return (
    <section aria-label={heading} className="flex flex-col gap-1.5 border-t border-border pt-3">
      <h2 className="label-mono text-muted-foreground">
        {heading} ({backlinks.length})
      </h2>
      <ul className="flex flex-wrap gap-2">
        {backlinks.map((page) => (
          <li key={page.id}>
            <Link
              to="/w/$workspaceId/wiki/$pageId"
              params={{ workspaceId, pageId: page.id }}
              className="inline-block max-w-64 truncate border border-border bg-card px-2 py-1 text-sm hover:border-foreground"
            >
              {page.title}
            </Link>
          </li>
        ))}
      </ul>
    </section>
  );
}
