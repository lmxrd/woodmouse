import { cn } from "../lib/utils";

export function initials(name: string): string {
  return name
    .split(/\s+/)
    .filter(Boolean)
    .slice(0, 2)
    .map((part) => part[0].toUpperCase())
    .join("");
}

/** A round initials badge. `size` is in Tailwind units: "sm" 20px, "md" 24px. */
export function UserAvatar({
  name,
  size = "sm",
  className,
}: {
  name: string;
  size?: "sm" | "md";
  className?: string;
}) {
  return (
    <span
      title={name}
      className={cn(
        "grid shrink-0 place-items-center rounded-full bg-secondary font-medium text-secondary-foreground",
        size === "sm" ? "size-5 text-[9px]" : "size-6 text-[10px]",
        className,
      )}
    >
      {initials(name)}
    </span>
  );
}

/** Overlapping avatars for an assignee list. */
export function AvatarStack({ names }: { names: string[] }) {
  return (
    <span className="flex">
      {names.map((name) => (
        <UserAvatar key={name} name={name} className="-ml-1 ring-2 ring-card first:ml-0" />
      ))}
    </span>
  );
}
