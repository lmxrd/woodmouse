/** Joins class names, skipping falsy values — handy for conditional styles. */
export function cn(...classes: Array<string | false | null | undefined>): string {
  return classes.filter(Boolean).join(" ");
}
