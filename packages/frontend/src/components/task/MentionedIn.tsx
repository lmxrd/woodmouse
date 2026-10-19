import { useQuery } from "@tanstack/react-query";
import { taskBacklinksOptions } from "woodmouse-sdk/react-query";

import { Backlinks } from "../wiki/Backlinks";

/** The wiki pages that mention this task. */
export function MentionedIn({ taskId, workspaceId }: { taskId: string; workspaceId: string }) {
  const { data: pages = [] } = useQuery(taskBacklinksOptions({ path: { id: taskId } }));
  return <Backlinks workspaceId={workspaceId} backlinks={pages} heading="Mentioned in" />;
}
