import { useState } from "react";

import { formatDuration, parseDuration } from "../../lib/duration";
import { Input } from "../ui/input";

/**
 * A work-time input (`1h 30m`, `90`, `1:30`) bound to minutes. It saves when you leave the field or press
 * Enter; something it cannot read is flagged and not saved.
 */
export function DurationField({
  id,
  value,
  onChange,
  placeholder,
}: {
  id: string;
  value: number | null | undefined;
  onChange: (minutes: number | null) => void;
  placeholder?: string;
}) {
  const shown = formatDuration(value);
  const [draft, setDraft] = useState<string | null>(null);
  const invalid = draft !== null && parseDuration(draft) === undefined;

  const commit = () => {
    if (draft === null) return;
    const parsed = parseDuration(draft);
    if (parsed !== undefined) {
      if ((parsed ?? null) !== (value ?? null)) onChange(parsed);
      setDraft(null);
    }
  };

  return (
    <Input
      id={id}
      inputMode="text"
      value={draft ?? shown}
      placeholder={placeholder ?? "e.g. 1h 30m"}
      aria-invalid={invalid}
      onChange={(e) => setDraft(e.target.value)}
      onBlur={() => {
        commit();
        if (!invalid) setDraft(null);
      }}
      onKeyDown={(e) => {
        if (e.key === "Enter") commit();
        if (e.key === "Escape") setDraft(null);
      }}
    />
  );
}
