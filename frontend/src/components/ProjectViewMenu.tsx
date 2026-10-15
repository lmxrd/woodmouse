import type { ProjectView } from "../lib/projectSearch";
import { Button } from "./ui/button";
import {
  DropdownMenu,
  DropdownMenuContent,
  DropdownMenuRadioGroup,
  DropdownMenuRadioItem,
  DropdownMenuTrigger,
} from "./ui/dropdown-menu";

const VIEWS: { value: ProjectView; label: string }[] = [
  { value: "board", label: "Board" },
  { value: "list", label: "List" },
];

/** Picks how the project's tasks are shown. Add new views (timeline, Gantt) to `VIEWS`. */
export function ProjectViewMenu({
  value,
  onChange,
}: {
  value: ProjectView;
  onChange: (view: ProjectView) => void;
}) {
  return (
    <DropdownMenu>
      <DropdownMenuTrigger asChild>
        <Button type="button" variant="outline" size="sm" aria-label="View">
          {VIEWS.find((v) => v.value === value)?.label} ▾
        </Button>
      </DropdownMenuTrigger>
      <DropdownMenuContent align="end">
        <DropdownMenuRadioGroup value={value} onValueChange={(v) => onChange(v as ProjectView)}>
          {VIEWS.map((view) => (
            <DropdownMenuRadioItem key={view.value} value={view.value}>
              {view.label}
            </DropdownMenuRadioItem>
          ))}
        </DropdownMenuRadioGroup>
      </DropdownMenuContent>
    </DropdownMenu>
  );
}
