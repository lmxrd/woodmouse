import { Select, SelectContent, SelectItem, SelectTrigger, SelectValue } from "../ui/select";

const ANY = "__any__";

/** A compact filter dropdown with an "any" choice; `value` undefined means no filter. */
export function FilterSelect({
  label,
  anyLabel,
  value,
  options,
  onChange,
}: {
  label: string;
  anyLabel: string;
  value: string | undefined;
  options: { value: string; label: string }[];
  onChange: (value: string | undefined) => void;
}) {
  return (
    <Select value={value ?? ANY} onValueChange={(v) => onChange(v === ANY ? undefined : v)}>
      <SelectTrigger
        size="sm"
        aria-label={label}
        className={value ? "border-foreground" : "text-muted-foreground"}
      >
        <SelectValue />
      </SelectTrigger>
      <SelectContent>
        <SelectItem value={ANY}>{anyLabel}</SelectItem>
        {options.map((o) => (
          <SelectItem key={o.value} value={o.value}>
            {o.label}
          </SelectItem>
        ))}
      </SelectContent>
    </Select>
  );
}
