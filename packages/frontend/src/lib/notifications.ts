import type { NotificationView } from "woodmouse-sdk";

const str = (v: unknown): string => (typeof v === "string" ? v : "");

/** One line saying what happened, in the words of the notification's kind. */
export function describeNotification(n: Pick<NotificationView, "kind" | "actor" | "data">): string {
  const who = n.actor ? n.actor.name.trim() || n.actor.email : "Someone";
  const title = str(n.data.title) || "something";
  switch (n.kind) {
    case "task_assigned":
      return `${who} assigned you to "${title}"`;
    case "comment_added":
      return `${who} commented on "${title}"`;
    case "mentioned":
      return `${who} mentioned a task you follow in the page "${title}"`;
    case "task_changed":
      return `${who} changed "${title}" (${str(n.data.what) || "updated"})`;
    case "page_changed":
      return `${who} edited the page "${title}"`;
    case "due_soon":
      return `"${title}" is due tomorrow`;
    case "overdue":
      return `"${title}" is overdue`;
    default:
      return `${who} did something with "${title}"`;
  }
}

/** A short excerpt to show under the line (a comment). */
export function notificationQuote(n: Pick<NotificationView, "kind" | "data">): string | undefined {
  return n.kind === "comment_added" ? str(n.data.preview) || undefined : undefined;
}
