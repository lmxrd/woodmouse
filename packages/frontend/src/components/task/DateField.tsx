import { toDateInput, fromDateInput } from "../../lib/dates";
import { Button } from "../ui/button";
import { Input } from "../ui/input";

/** A native date input (good touch UI for free) bound to an API day value (`2026-10-16T00:00:00Z`), with a clear button. */
export function DateField({
  id,
  value,
  onChange,
}: {
  id: string;
  value: string | null | undefined;
  onChange: (value: string | null) => void;
}) {
  return (
    <div className="flex gap-1">
      <Input
        id={id}
        type="date"
        value={toDateInput(value)}
        onChange={(e) => onChange(fromDateInput(e.target.value))}
      />
      {value != null ? (
        <Button
          type="button"
          variant="ghost"
          size="icon"
          aria-label="Clear date"
          onClick={() => onChange(null)}
        >
          ✕
        </Button>
      ) : null}
    </div>
  );
}
