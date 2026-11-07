import { useEffect, useState } from "react";
import { pingPresence } from "woodmouse-sdk";
import type { Editor } from "woodmouse-sdk";

const PRESENCE_EVERY_MS = 10_000;

/**
 * Tell the server this page is open for editing (and whether without live sync), and learn who else has
 * it open. Only used to warn about people editing without live sync; live editors see each other through
 * the cursors.
 */
export function usePresence(pageId: string, enabled: boolean, fallback: boolean): Editor[] {
  const [others, setOthers] = useState<Editor[]>([]);
  useEffect(() => {
    if (!enabled) return;
    const ping = async () => {
      if (document.visibilityState !== "visible") return;
      const { data } = await pingPresence({ path: { id: pageId }, body: { fallback } });
      if (data) setOthers(data);
    };
    void ping();
    const id = setInterval(() => void ping(), PRESENCE_EVERY_MS);
    return () => clearInterval(id);
  }, [pageId, enabled, fallback]);
  return others;
}
