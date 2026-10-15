import { toDateInput, fromDateInput } from "../../lib/dates";
import { Button } from "../ui/button";
import { Input } from "../ui/input";

/** A native date input (good touch UI for free) bound to epoch seconds, with a clear button. */
export function DateField({
  id,
  value,
  onChange,
}: {
  id: string;
  value: number | null | undefined;
  onChange: (value: number | null) => void;
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
