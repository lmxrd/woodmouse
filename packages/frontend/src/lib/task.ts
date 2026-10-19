import type { TaskResponse } from "woodmouse-sdk";

/** A task is done once it has a completion time; the server sets it when `is_completed: true` is sent. */
export function isDone(task: Pick<TaskResponse, "completed_at">): boolean {
  return task.completed_at != null;
}
