import { useEffect, useState } from "react";
import type { WebsocketProvider } from "y-websocket";

interface Collaborator {
  clientId: number;
  name: string;
  color: string;
}

/** Who else has the page open, from the live connection's presence (the same names as the cursors). */
export function Collaborators({ provider }: { provider: WebsocketProvider }) {
  const [others, setOthers] = useState<Collaborator[]>([]);
  useEffect(() => {
    const awareness = provider.awareness;
    const refresh = () => {
      const seen = new Set<string>();
      const list: Collaborator[] = [];
      awareness.getStates().forEach((state, clientId) => {
        const user = state.user as { name?: string; color?: string } | undefined;
        if (clientId === awareness.clientID || !user?.name) return;
        // One entry per person, even with the page open in two tabs.
        if (seen.has(user.name)) return;
        seen.add(user.name);
        list.push({ clientId, name: user.name, color: user.color ?? "#1f4d3a" });
      });
      setOthers(list);
    };
    refresh();
    awareness.on("change", refresh);
    return () => awareness.off("change", refresh);
  }, [provider]);

  if (others.length === 0) return <span />;
  return (
    <ul aria-label="Also editing" className="flex flex-wrap items-center gap-1">
      {others.map((person) => (
        <li
          key={person.clientId}
          className="flex items-center gap-1.5 border border-border px-1.5 py-0.5 text-xs"
        >
          <span
            aria-hidden
            className="size-2 rounded-full"
            style={{ backgroundColor: person.color }}
          />
          {person.name}
        </li>
      ))}
    </ul>
  );
}
