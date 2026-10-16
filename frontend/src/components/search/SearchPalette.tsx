import { useEffect, useState } from "react";
import { useQuery } from "@tanstack/react-query";
import { useNavigate } from "@tanstack/react-router";
import type { SearchHit } from "woodmouse-client";
import { searchOptions } from "woodmouse-client/react-query";

import { cn } from "../../lib/utils";
import { Checkbox } from "../ui/checkbox";
import {
  CommandDialog,
  CommandEmpty,
  CommandGroup,
  CommandInput,
  CommandItem,
  CommandList,
} from "../ui/command";

const RESULT_LIMIT = 20;
const TYPING_PAUSE_MS = 150;

/** ⌘K search over the pages and tasks of this workspace, or of every workspace you belong to. */
export function SearchPalette({
  workspaceId,
  open,
  onOpenChange,
}: {
  workspaceId: string;
  open: boolean;
  onOpenChange: (open: boolean) => void;
}) {
  const navigate = useNavigate();
  const [text, setText] = useState("");
  const [typed, setTyped] = useState("");
  const [everywhere, setEverywhere] = useState(false);

  // Search once typing pauses, not on every keystroke.
  useEffect(() => {
    const timer = setTimeout(() => setTyped(text.trim()), TYPING_PAUSE_MS);
    return () => clearTimeout(timer);
  }, [text]);

  const { data: hits = [], isFetching } = useQuery({
    ...searchOptions({
      path: { id: workspaceId },
      query: { q: typed, limit: RESULT_LIMIT, all_workspaces: everywhere },
    }),
    enabled: open && typed !== "",
    placeholderData: (previous) => previous,
  });
  const shown = typed === "" ? [] : hits;
  // Keep the highlight on a result that exists: the first one whenever the results change.
  const [highlighted, setHighlighted] = useState("");
  const active = shown.some((h) => h.id === highlighted) ? highlighted : (shown[0]?.id ?? "");
  const pages = shown.filter((h) => h.kind === "page");
  const tasks = shown.filter((h) => h.kind === "task");

  const close = (next: boolean) => {
    onOpenChange(next);
    if (!next) {
      setText("");
      setTyped("");
    }
  };
  const open_ = (hit: SearchHit) => {
    close(false);
    if (hit.kind === "page") {
      void navigate({
        to: "/w/$workspaceId/wiki/$pageId",
        params: { workspaceId: hit.workspace_id, pageId: hit.id },
      });
    } else if (hit.project_id) {
      void navigate({
        to: "/w/$workspaceId/p/$projectId/t/$taskId",
        params: { workspaceId: hit.workspace_id, projectId: hit.project_id, taskId: hit.id },
      });
    }
  };

  const row = (hit: SearchHit) => (
    <CommandItem key={hit.id} value={hit.id} onSelect={() => open_(hit)} className="items-start">
      <span className="flex min-w-0 flex-1 flex-col leading-tight">
        <span className={cn("truncate", hit.is_completed && "text-muted-foreground line-through")}>
          {hit.title}
        </span>
        {hit.snippet && hit.snippet !== hit.title ? (
          <span className="truncate text-xs text-muted-foreground">{hit.snippet}</span>
        ) : null}
      </span>
      {everywhere ? (
        <span className="label-mono shrink-0 text-muted-foreground">{hit.workspace_name}</span>
      ) : null}
    </CommandItem>
  );

  return (
    <CommandDialog
      open={open}
      onOpenChange={close}
      title="Search"
      description="Search pages and tasks"
      showCloseButton={false}
      shouldFilter={false}
      value={active}
      onValueChange={setHighlighted}
      className="top-[12vh] translate-y-0 rounded-none border-foreground sm:max-w-xl"
    >
      <CommandInput
        value={text}
        onValueChange={setText}
        placeholder={everywhere ? "Search all workspaces…" : "Search pages and tasks…"}
      />
      <CommandList className="max-h-[50vh]">
        {typed !== "" && !isFetching && shown.length === 0 ? (
          <CommandEmpty>No results for “{typed}”.</CommandEmpty>
        ) : null}
        {typed === "" ? (
          <p className="px-3 py-6 text-center text-sm text-muted-foreground">
            Type to search titles and text.
          </p>
        ) : null}
        {pages.length > 0 ? <CommandGroup heading="Pages">{pages.map(row)}</CommandGroup> : null}
        {tasks.length > 0 ? <CommandGroup heading="Tasks">{tasks.map(row)}</CommandGroup> : null}
      </CommandList>
      <label className="flex items-center gap-2 border-t border-border px-3 py-2 text-sm text-muted-foreground">
        <Checkbox
          checked={everywhere}
          onCheckedChange={(checked) => setEverywhere(checked === true)}
        />
        Search all my workspaces
      </label>
    </CommandDialog>
  );
}
