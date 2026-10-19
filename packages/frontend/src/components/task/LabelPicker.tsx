import { useState } from "react";
import type { Label } from "woodmouse-sdk";

import { LabelTag } from "../common/LabelTag";
import { Button } from "../ui/button";
import { Checkbox } from "../ui/checkbox";
import { Input } from "../ui/input";
import { Popover, PopoverContent, PopoverTrigger } from "../ui/popover";

/** Label chips plus a popover to tick workspace labels or create a new one. */
export function LabelPicker({
  labels,
  selected,
  onChange,
  onCreate,
}: {
  labels: Label[];
  selected: Label[];
  onChange: (labelIds: string[]) => void;
  /** Create a workspace label; resolves with the new label so it can be attached. */
  onCreate: (name: string) => Promise<Label>;
}) {
  const [name, setName] = useState("");
  const selectedIds = new Set(selected.map((l) => l.id));
  const toggle = (id: string) =>
    onChange(selectedIds.has(id) ? [...selectedIds].filter((x) => x !== id) : [...selectedIds, id]);

  return (
    <div className="flex flex-wrap items-center gap-1">
      {selected.map((label) => (
        <LabelTag key={label.id} name={label.name} color={label.color} />
      ))}
      <Popover>
        <PopoverTrigger asChild>
          <Button type="button" variant="outline" size="xs" aria-label="Edit labels">
            {selected.length === 0 ? "Add label" : "+"}
          </Button>
        </PopoverTrigger>
        <PopoverContent align="start" className="w-56 p-1">
          {labels.map((label) => (
            <label
              key={label.id}
              className="flex cursor-pointer items-center gap-2 px-2 py-1.5 text-sm hover:bg-accent"
            >
              <Checkbox
                checked={selectedIds.has(label.id)}
                onCheckedChange={() => toggle(label.id)}
              />
              <span className="min-w-0 flex-1 truncate">{label.name}</span>
            </label>
          ))}
          <form
            className="p-1"
            onSubmit={async (e) => {
              e.preventDefault();
              const trimmed = name.trim();
              if (!trimmed) return;
              setName("");
              const created = await onCreate(trimmed);
              onChange([...selectedIds, created.id]);
            }}
          >
            <Input
              value={name}
              placeholder="New label"
              aria-label="New label name"
              onChange={(e) => setName(e.target.value)}
            />
          </form>
        </PopoverContent>
      </Popover>
    </div>
  );
}
