import { Input } from "./ui/input";

/** An input that replaces a title while renaming: Enter or blur saves, Escape cancels. */
export function RenameInput({
  value,
  label,
  className,
  onSave,
  onCancel,
}: {
  value: string;
  label: string;
  className?: string;
  onSave: (name: string) => void;
  onCancel: () => void;
}) {
  const finish = (next: string) => {
    const name = next.trim();
    if (name && name !== value) onSave(name);
    else onCancel();
  };
  return (
    <Input
      autoFocus
      defaultValue={value}
      aria-label={label}
      className={className}
      onFocus={(e) => e.target.select()}
      onBlur={(e) => finish(e.target.value)}
      onKeyDown={(e) => {
        if (e.key === "Enter") e.currentTarget.blur();
        if (e.key === "Escape") {
          e.currentTarget.value = value;
          e.currentTarget.blur();
        }
      }}
    />
  );
}
