import { useQuery } from "@tanstack/react-query";
import { adminIntegrityOptions } from "woodmouse-sdk/react-query";

import { Button } from "../ui/button";

/** The integrity check: rows pointing at nothing. */
export function MaintenanceTab() {
  // Only runs when asked: it reads every table.
  const integrity = useQuery({ ...adminIntegrityOptions(), enabled: false, staleTime: 0 });

  return (
    <div className="flex flex-col gap-6">
      <section className="flex flex-col gap-3 border border-border bg-card p-5">
        <h2 className="label-mono text-muted-foreground">Integrity check</h2>
        <p className="text-sm text-muted-foreground">
          Looks for rows that point at something that no longer exists.
        </p>
        <div>
          <Button onClick={() => void integrity.refetch()} disabled={integrity.isFetching}>
            Run check
          </Button>
        </div>
        {integrity.data ? (
          integrity.data.length === 0 ? (
            <p className="text-sm">All good: no problems found.</p>
          ) : (
            <ul className="flex flex-col gap-1 font-mono text-xs">
              {integrity.data.map((p) => (
                <li key={`${p.table}:${p.id}:${p.relation}`}>
                  {p.table} {p.id}: {p.relation}
                </li>
              ))}
            </ul>
          )
        ) : null}
      </section>
    </div>
  );
}
