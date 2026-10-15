import type { Priority } from "woodmouse-client";

export const PRIORITIES: Priority[] = ["low", "medium", "high", "urgent"];

/** Short badge text, "P1" (low) to "P4" (urgent). */
export const PRIORITY_BADGE: Record<Priority, string> = {
  low: "P1",
  medium: "P2",
  high: "P3",
  urgent: "P4",
};

export const PRIORITY_NAME: Record<Priority, string> = {
  low: "Low",
  medium: "Medium",
  high: "High",
  urgent: "Urgent",
};

/** High and urgent get the signal color. */
export function isHotPriority(priority: Priority): boolean {
  return priority === "high" || priority === "urgent";
}
