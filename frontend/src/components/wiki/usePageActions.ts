import { useMutation, useQueryClient } from "@tanstack/react-query";
import { useNavigate } from "@tanstack/react-router";
import {
  createPageMutation,
  deletePageMutation,
  updatePageMutation,
} from "woodmouse-client/react-query";

/** Creating, moving and deleting pages; every change refreshes the tree. */
export function usePageActions(workspaceId: string) {
  const queryClient = useQueryClient();
  const navigate = useNavigate();
  const refresh = () => queryClient.invalidateQueries();

  const create = useMutation({
    ...createPageMutation(),
    onSuccess: async (page) => {
      await refresh();
      void navigate({
        to: "/w/$workspaceId/wiki/$pageId",
        params: { workspaceId, pageId: page.id },
      });
    },
  });
  const update = useMutation({ ...updatePageMutation(), onSuccess: refresh });
  const remove = useMutation({ ...deletePageMutation(), onSuccess: refresh });

  return {
    /** A new "Untitled" page (top level, or below `parentId`), opened with its title ready to edit. */
    createPage: (parentId?: string) =>
      create.mutate({ path: { id: workspaceId }, body: { parent_id: parentId } }),
    movePage: (id: string, body: { parent_id?: string | null; position?: number }) =>
      update.mutate({ path: { id }, body }),
    deletePage: (id: string, afterDelete?: () => void) =>
      remove.mutate({ path: { id } }, { onSuccess: afterDelete }),
  };
}
