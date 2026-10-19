import type { ActivityView } from "woodmouse-sdk";

import { userLabel } from "./user";

const str = (v: unknown): string => (typeof v === "string" ? v : "");

/** A one-line plain-text excerpt of markdown: list markers, emphasis and code ticks removed. */
export function plainText(markdown: string): string {
  return markdown
    .split("\n")
    .map((line) => line.replace(/^\s*(?:[-*+]|\d+\.|>|#{1,6})\s+/, ""))
    .join(" ")
    .replace(/[*_`~]/g, "")
    .replace(/\s+/g, " ")
    .trim();
}
const KIND_PHRASE: Record<string, string> = {
  blocks: "blocks",
  relates_to: "relates to",
  duplicates: "duplicates",
};

/** What happened, as the words after the actor's name; `quote` is an optional excerpt (a comment). */
export function describeActivity(
  entry: ActivityView,
  formatDay: (day: string) => string,
): { text: string; quote?: string } {
  const d = entry.data;
  const title = str(d.title);
  switch (entry.kind) {
    case "task_created":
      return { text: `created "${title}"` };
    case "subtask_added":
      return { text: `added the subtask "${title}" to "${str(d.parent_title)}"` };
    case "task_renamed":
      return { text: `renamed "${str(d.from)}" to "${str(d.to)}"` };
    case "task_completed":
      return { text: `completed "${title}"` };
    case "task_reopened":
      return { text: `reopened "${title}"` };
    case "task_moved":
      return { text: `moved "${title}" from ${str(d.from)} to ${str(d.to)}` };
    case "due_changed":
      return {
        text:
          typeof d.to === "string"
            ? `set the due date of "${title}" to ${formatDay(d.to)}`
            : `cleared the due date of "${title}"`,
      };
    case "priority_changed":
      return { text: `set the priority of "${title}" to ${str(d.to)}` };
    case "assigned":
      return { text: `assigned "${title}" to ${Array.isArray(d.who) ? d.who.join(", ") : ""}` };
    case "unassigned":
      return {
        text: `unassigned ${Array.isArray(d.who) ? d.who.join(", ") : ""} from "${title}"`,
      };
    case "comment_added":
      return { text: `commented on "${title}"`, quote: plainText(str(d.preview)) };
    case "file_attached":
      return { text: `attached ${str(d.file)} to "${title}"` };
    case "link_added":
      return {
        text: `linked "${title}": it ${KIND_PHRASE[str(d.kind)] ?? str(d.kind)} "${str(d.other)}"`,
      };
    case "file_version_added":
      return { text: `uploaded version ${String(d.version)} of ${str(d.file)}` };
    case "sprint_created":
      return { text: `created the sprint "${str(d.name)}"` };
    case "sprint_completed":
      return {
        text: `completed the sprint "${str(d.name)}"${
          typeof d.moved === "number" && d.moved > 0 ? ` (${d.moved} unfinished moved on)` : ""
        }`,
      };
    case "page_file_attached":
      return { text: `attached ${str(d.file)} to the page "${title}"` };
    case "page_file_detached":
      return { text: `removed ${str(d.file)} from the page "${title}"` };
    case "task_deleted":
      return { text: `deleted "${title}"` };
    case "task_restored":
      return { text: `restored "${title}"` };
    default:
      return { text: entry.kind.replaceAll("_", " ") };
  }
}

export function actorLabel(entry: ActivityView): string {
  return entry.actor ? userLabel(entry.actor) : "Someone";
}
