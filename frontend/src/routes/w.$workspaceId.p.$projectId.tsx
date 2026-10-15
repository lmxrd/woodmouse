import { createFileRoute, Outlet, useNavigate } from "@tanstack/react-router";
import { useState } from "react";
import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import {
  deleteProjectMutation,
  getProjectOptions,
  listProjectsQueryKey,
  updateProjectMutation,
} from "woodmouse-client/react-query";

import { ActionMenu } from "../components/ActionMenu";
import { Board } from "../components/Board";
import { CalendarView } from "../components/calendar/CalendarView";
import { ConfirmDialog } from "../components/ConfirmDialog";
import { TimelineView } from "../components/timeline/TimelineView";
import { ProjectViewMenu } from "../components/ProjectViewMenu";
import { RenameInput } from "../components/RenameInput";
import { TaskListView } from "../components/TaskListView";
import { useWorkspaceRole } from "../lib/useWorkspaceRole";
import { validateProjectSearch } from "../lib/projectSearch";

export const Route = createFileRoute("/w/$workspaceId/p/$projectId")({
  validateSearch: validateProjectSearch,
  component: ProjectPage,
});

function ProjectPage() {
  const { workspaceId, projectId } = Route.useParams();
  const search = Route.useSearch();
  const { view = "board" } = search;
  const navigate = Route.useNavigate();
  const routerNavigate = useNavigate();
  const { data: project, isError } = useQuery({
    ...getProjectOptions({ path: { id: projectId } }),
    retry: false, // a missing project will not appear on retry
  });
  const { canWrite } = useWorkspaceRole(workspaceId);
  const queryClient = useQueryClient();
  const rename = useMutation({
    ...updateProjectMutation(),
    onSuccess: () => queryClient.invalidateQueries(),
  });
  const remove = useMutation({
    ...deleteProjectMutation(),
    onSuccess: async () => {
      // Refresh only the project list before leaving (the workspace index redirects to its first
      // project, so a stale cache would send us back here); everything else refreshes after.
      await queryClient.invalidateQueries({
        queryKey: listProjectsQueryKey({ path: { workspace_id: workspaceId } }),
      });
      void navigate({ to: "/w/$workspaceId", params: { workspaceId } });
      void queryClient.invalidateQueries();
    },
  });
  const [renaming, setRenaming] = useState(false);
  const [confirmDelete, setConfirmDelete] = useState(false);

  if (isError) {
    return (
      <div className="flex flex-col gap-2 px-6 py-8">
        <h1 className="font-display text-2xl font-bold">This project doesn't exist</h1>
        <p className="text-sm text-muted-foreground">
          It may have been deleted. Pick another project from the sidebar.
        </p>
      </div>
    );
  }

  return (
    <>
      <header className="flex items-end justify-between gap-4 border-b border-border px-4 pt-5 pb-4 sm:px-6">
        <div>
          <p className="label-mono text-muted-foreground">Project</p>
          {renaming && project ? (
            <RenameInput
              label="Project name"
              value={project.name}
              className="h-10 font-display text-2xl font-bold"
              onCancel={() => setRenaming(false)}
              onSave={(name) => {
                rename.mutate({ path: { id: projectId }, body: { name } });
                setRenaming(false);
              }}
            />
          ) : (
            <h1 className="font-display text-2xl font-bold">{project?.name ?? "\u00a0"}</h1>
          )}
        </div>
        <div className="flex items-center gap-1">
          <ProjectViewMenu
            value={view}
            projectId={projectId}
            workspaceId={workspaceId}
            search={search}
            onChange={(next) => void navigate({ search: (prev) => ({ ...prev, view: next }) })}
            onApply={(params) => void navigate({ search: validateProjectSearch(params) })}
          />
          {canWrite ? (
            <ActionMenu
              label="Project actions"
              items={[
                { label: "Rename project", onSelect: () => setRenaming(true) },
                {
                  label: "Activity",
                  onSelect: () =>
                    void routerNavigate({
                      to: "/w/$workspaceId/activity",
                      params: { workspaceId },
                      search: { project: projectId },
                    }),
                },
                {
                  label: "Delete project…",
                  destructive: true,
                  onSelect: () => setConfirmDelete(true),
                },
              ]}
            />
          ) : null}
        </div>
      </header>
      {view === "list" ? (
        <TaskListView workspaceId={workspaceId} projectId={projectId} />
      ) : view === "calendar" ? (
        <CalendarView workspaceId={workspaceId} projectId={projectId} />
      ) : view === "timeline" ? (
        <TimelineView workspaceId={workspaceId} projectId={projectId} />
      ) : (
        <Board projectId={projectId} workspaceId={workspaceId} />
      )}
      <Outlet />
      <ConfirmDialog
        open={confirmDelete}
        onOpenChange={setConfirmDelete}
        title={`Delete "${project?.name ?? ""}"?`}
        description="This also deletes all of its lists and tasks. This can't be undone."
        confirmLabel="Delete project"
        onConfirm={() => remove.mutate({ path: { id: projectId } })}
      />
    </>
  );
}
