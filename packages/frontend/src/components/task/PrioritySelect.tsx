import type { Priority } from "woodmouse-client";

import { PRIORITIES, PRIORITY_BADGE, PRIORITY_NAME, isHotPriority } from "../../lib/priority";
import { cn } from "../../lib/utils";
import { Select, SelectContent, SelectItem, SelectTrigger, SelectValue } from "../ui/select";

export function PrioritySelect({
  value,
  onChange,
}: {
  value: Priority;
  onChange: (value: Priority) => void;
}) {
  return (
    <Select value={value} onValueChange={(v) => onChange(v as Priority)}>
      <SelectTrigger
        aria-label="Priority"
        className={cn("w-full", isHotPriority(value) && "font-medium text-signal")}
      >
        <SelectValue />
      </SelectTrigger>
      <SelectContent>
        {PRIORITIES.map((p) => (
          <SelectItem key={p} value={p}>
            {PRIORITY_BADGE[p]} {PRIORITY_NAME[p]}
          </SelectItem>
        ))}
      </SelectContent>
    </Select>
  );
}
