import { useState } from "react";
import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import type { List, TaskResponse } from "woodmouse-client";
import {
  createListMutation,
  createTaskMutation,
  deleteListMutation,
  listListsOptions,
  listTasksOptions,
  updateListMutation,
  updateTaskMutation,
} from "woodmouse-client/react-query";

import { useWorkspaceRole } from "../lib/useWorkspaceRole";
import { ActionMenu } from "./ActionMenu";
import { ConfirmDialog } from "./ConfirmDialog";
import { InlineAdd } from "./InlineAdd";
import { RenameInput } from "./RenameInput";
import { BoardColumn } from "./board/BoardColumn";
import { TaskCard } from "./board/TaskCard";
import { Button } from "./ui/button";

/** How many completed tasks the Done column shows before "Show all". */
const DONE_LIMIT = 20;

/**
 * One column per list (open tasks only) plus a fixed Done column holding every completed task of the
 * project. Completion is a task state, not a list: dropping a card on Done completes it in place, and
 * dropping a completed card on a list reopens it and moves it there.
 */
export function Board({ projectId, workspaceId }: { projectId: string; workspaceId: string }) {
  const queryClient = useQueryClient();
  const { data: lists } = useQuery(listListsOptions({ path: { project_id: projectId } }));
  const { data: allTasks } = useQuery(
    listTasksOptions({ path: { workspace_id: workspaceId }, query: { project_id: projectId } }),
  );

  const refresh = () => queryClient.invalidateQueries();
  const createList = useMutation({ ...createListMutation(), onSuccess: refresh });
  const createTask = useMutation({ ...createTaskMutation(), onSuccess: refresh });
  const updateTask = useMutation({ ...updateTaskMutation(), onSuccess: refresh });
  const renameList = useMutation({ ...updateListMutation(), onSuccess: refresh });
  const deleteList = useMutation({ ...deleteListMutation(), onSuccess: refresh });
  const { canWrite } = useWorkspaceRole(workspaceId);
  const [renaming, setRenaming] = useState<string | null>(null);
  const [deleting, setDeleting] = useState<List | null>(null);
  const [showAllDone, setShowAllDone] = useState(false);

  const tasks = allTasks ?? [];
  const openIn = (listId: string) => tasks.filter((t) => t.list_id === listId && !t.is_completed);
  const completed = tasks
    .filter((t) => t.is_completed)
    .sort((a, b) => (b.completed_at ?? 0) - (a.completed_at ?? 0));
  const visibleDone = showAllDone ? completed : completed.slice(0, DONE_LIMIT);

  const dropOnList = (taskId: string, listId: string) => {
    const task = tasks.find((t) => t.id === taskId);
    if (!task) return;
    const body: { list_id?: string; is_completed?: boolean } = {};
    if (task.list_id !== listId) body.list_id = listId;
    if (task.is_completed) body.is_completed = false;
    if (Object.keys(body).length > 0) updateTask.mutate({ path: { id: taskId }, body });
  };
  const dropOnDone = (taskId: string) => {
    const task = tasks.find((t) => t.id === taskId);
    if (task && !task.is_completed) {
      updateTask.mutate({ path: { id: taskId }, body: { is_completed: true } });
    }
  };
  const toggleCompleted = (task: TaskResponse, isCompleted: boolean) =>
    updateTask.mutate({ path: { id: task.id }, body: { is_completed: isCompleted } });

  const deletingCount = tasks.filter((t) => t.list_id === deleting?.id).length;
  const deleteDescription =
    deletingCount === 0
      ? "This can't be undone."
      : `This also deletes its ${deletingCount} ${deletingCount === 1 ? "task" : "tasks"}, including completed ones. This can't be undone.`;

  const card = (task: TaskResponse) => (
    <TaskCard
      key={task.id}
      task={task}
      workspaceId={workspaceId}
      projectId={projectId}
      editable={canWrite}
      onToggleCompleted={toggleCompleted}
    />
  );

  return (
    <div className="flex snap-x snap-mandatory scroll-px-4 items-start gap-4 overflow-x-auto p-4 sm:snap-none sm:p-6">
      {lists?.map((list) => {
        const open = openIn(list.id);
        return (
          <BoardColumn
            key={list.id}
            label={list.name}
            onDropTask={canWrite ? (taskId) => dropOnList(taskId, list.id) : undefined}
            header={
              <>
                {renaming === list.id ? (
                  <RenameInput
                    label={`Rename ${list.name}`}
                    value={list.name}
                    className="h-7 flex-1"
                    onCancel={() => setRenaming(null)}
                    onSave={(name) => {
                      renameList.mutate({ path: { id: list.id }, body: { name } });
                      setRenaming(null);
                    }}
                  />
                ) : (
                  <h2 className="label-mono flex min-w-0 flex-1 justify-between gap-2">
                    <span className="truncate">{list.name}</span>
                    <span className="text-muted-foreground">{open.length}</span>
                  </h2>
                )}
                {canWrite && renaming !== list.id ? (
                  <ActionMenu
                    label={`Actions for ${list.name}`}
                    items={[
                      { label: "Rename list", onSelect: () => setRenaming(list.id) },
                      {
                        label: "Delete list…",
                        destructive: true,
                        onSelect: () => setDeleting(list),
                      },
                    ]}
                  />
                ) : null}
              </>
            }
          >
            {open.map(card)}
            {canWrite ? (
              <InlineAdd
                label="Add task"
                placeholder="Task title"
                onSubmit={(title) =>
                  createTask.mutate({ path: { list_id: list.id }, body: { title } })
                }
              />
            ) : null}
          </BoardColumn>
        );
      })}

      <BoardColumn
        label="Done"
        onDropTask={canWrite ? dropOnDone : undefined}
        header={
          <h2 className="label-mono flex min-w-0 flex-1 justify-between gap-2">
            <span>Done</span>
            <span className="text-muted-foreground">{completed.length}</span>
          </h2>
        }
      >
        {visibleDone.map(card)}
        {completed.length === 0 ? (
          <p className="border border-dashed border-border px-3 py-2 text-sm text-muted-foreground">
            Completed tasks show up here.
          </p>
        ) : null}
        {completed.length > DONE_LIMIT ? (
          <Button
            type="button"
            variant="ghost"
            size="sm"
            className="justify-start font-normal text-muted-foreground"
            onClick={() => setShowAllDone((v) => !v)}
          >
            {showAllDone ? "Show recent only" : `Show all ${completed.length}`}
          </Button>
        ) : null}
      </BoardColumn>

      {canWrite ? (
        <InlineAdd
          className="w-[160px] shrink-0"
          label="Add list"
          placeholder="List name"
          onSubmit={(name) =>
            createList.mutate({ path: { project_id: projectId }, body: { name } })
          }
        />
      ) : null}
      <ConfirmDialog
        open={deleting !== null}
        onOpenChange={(open) => !open && setDeleting(null)}
        title={`Delete "${deleting?.name ?? ""}"?`}
        description={deleteDescription}
        confirmLabel="Delete list"
        onConfirm={() => deleting && deleteList.mutate({ path: { id: deleting.id } })}
      />
    </div>
  );
}
