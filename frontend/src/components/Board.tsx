import { useState } from "react";
import {
  DndContext,
  DragOverlay,
  KeyboardSensor,
  PointerSensor,
  TouchSensor,
  closestCorners,
  useSensor,
  useSensors,
  type DragEndEvent,
  type DragStartEvent,
} from "@dnd-kit/core";
import {
  SortableContext,
  sortableKeyboardCoordinates,
  verticalListSortingStrategy,
} from "@dnd-kit/sortable";
import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import type { List, TaskResponse, UpdateTaskRequest } from "woodmouse-client";
import {
  createListMutation,
  createTaskMutation,
  deleteListMutation,
  listListsOptions,
  listTasksOptions,
  updateListMutation,
  updateTaskMutation,
} from "woodmouse-client/react-query";

import { positionBetween } from "../lib/position";
import { useWorkspaceRole } from "../lib/useWorkspaceRole";
import { ActionMenu } from "./ActionMenu";
import { ConfirmDialog } from "./ConfirmDialog";
import { InlineAdd } from "./InlineAdd";
import { RenameInput } from "./RenameInput";
import { BoardColumn } from "./board/BoardColumn";
import { SortableTaskCard, TaskCardOverlay } from "./board/TaskCard";
import { Button } from "./ui/button";

/** How many completed tasks the Done column shows before "Show all". */
const DONE_LIMIT = 20;

/**
 * One column per list (open tasks only) plus a fixed Done column holding every completed task of the
 * project. Completion is a task state, not a list: dropping a card on Done completes it in place, and
 * dropping a completed card on a list reopens it and moves it there. Cards are ordered by their
 * `position`; dropping between two cards gives the task the midpoint of their positions.
 */
export function Board({ projectId, workspaceId }: { projectId: string; workspaceId: string }) {
  const queryClient = useQueryClient();
  const { data: lists } = useQuery(listListsOptions({ path: { project_id: projectId } }));
  const tasksQuery = listTasksOptions({
    path: { workspace_id: workspaceId },
    query: { project_id: projectId },
  });
  const { data: allTasks } = useQuery(tasksQuery);

  const refresh = () => queryClient.invalidateQueries();
  const createList = useMutation({ ...createListMutation(), onSuccess: refresh });
  const createTask = useMutation({ ...createTaskMutation(), onSuccess: refresh });
  const renameList = useMutation({ ...updateListMutation(), onSuccess: refresh });
  const deleteList = useMutation({ ...deleteListMutation(), onSuccess: refresh });
  // Moves update the cache first so the card stays where it was dropped instead of jumping back.
  const updateTask = useMutation({
    ...updateTaskMutation(),
    onMutate: async ({ path, body }) => {
      await queryClient.cancelQueries({ queryKey: tasksQuery.queryKey });
      const previous = queryClient.getQueryData<TaskResponse[]>(tasksQuery.queryKey);
      queryClient.setQueryData<TaskResponse[]>(tasksQuery.queryKey, (old) =>
        old?.map((t) =>
          t.id === path.id
            ? {
                ...t,
                ...(body.list_id ? { list_id: body.list_id } : {}),
                ...(body.position != null ? { position: body.position } : {}),
                ...(body.is_completed != null
                  ? {
                      is_completed: body.is_completed,
                      completed_at: body.is_completed ? Math.floor(Date.now() / 1000) : null,
                    }
                  : {}),
              }
            : t,
        ),
      );
      return { previous };
    },
    onError: (_error, _vars, context) => {
      if (context?.previous) queryClient.setQueryData(tasksQuery.queryKey, context.previous);
    },
    onSettled: refresh,
  });
  const { canWrite } = useWorkspaceRole(workspaceId);
  const [renaming, setRenaming] = useState<string | null>(null);
  const [deleting, setDeleting] = useState<List | null>(null);
  const [showAllDone, setShowAllDone] = useState(false);
  const [dragging, setDragging] = useState<TaskResponse | null>(null);

  const sensors = useSensors(
    // A few pixels of movement start a drag, so plain clicks still open the task.
    useSensor(PointerSensor, { activationConstraint: { distance: 6 } }),
    // On touch screens a short press starts the drag, so the board still scrolls normally.
    useSensor(TouchSensor, { activationConstraint: { delay: 250, tolerance: 8 } }),
    useSensor(KeyboardSensor, { coordinateGetter: sortableKeyboardCoordinates }),
  );

  const tasks = allTasks ?? [];
  const openIn = (listId: string) =>
    tasks
      .filter((t) => t.list_id === listId && !t.is_completed)
      .sort((a, b) => a.position - b.position);
  const completed = tasks
    .filter((t) => t.is_completed)
    .sort((a, b) => (b.completed_at ?? 0) - (a.completed_at ?? 0));
  const visibleDone = showAllDone ? completed : completed.slice(0, DONE_LIMIT);

  const onDragStart = (event: DragStartEvent) =>
    setDragging(tasks.find((t) => t.id === event.active.id) ?? null);

  const onDragEnd = ({ active, over }: DragEndEvent) => {
    setDragging(null);
    const task = tasks.find((t) => t.id === active.id);
    if (!task || !over) return;

    // Where was it dropped: on a column (append) or on another card (before or after it)?
    const overId = String(over.id);
    const overTask = overId.startsWith("col:") ? undefined : tasks.find((t) => t.id === overId);
    const column = overId.startsWith("col:")
      ? overId.slice(4)
      : overTask
        ? overTask.is_completed
          ? "done"
          : overTask.list_id
        : undefined;
    if (!column) return;

    if (column === "done") {
      if (!task.is_completed) {
        updateTask.mutate({ path: { id: task.id }, body: { is_completed: true } });
      }
      return;
    }

    const siblings = openIn(column).filter((t) => t.id !== task.id);
    let index = siblings.length;
    if (overTask && overTask.id !== task.id) {
      const overIndex = siblings.findIndex((t) => t.id === overTask.id);
      const moved = active.rect.current.translated;
      const below = !!moved && moved.top + moved.height / 2 > over.rect.top + over.rect.height / 2;
      if (overIndex >= 0) index = overIndex + (below ? 1 : 0);
    }

    const sameSpot =
      task.list_id === column &&
      !task.is_completed &&
      openIn(column).findIndex((t) => t.id === task.id) === index;
    if (sameSpot) return;

    const body: UpdateTaskRequest = { position: positionBetween(siblings, index) };
    if (task.list_id !== column) body.list_id = column;
    if (task.is_completed) body.is_completed = false;
    updateTask.mutate({ path: { id: task.id }, body });
  };

  const toggleCompleted = (task: TaskResponse, isCompleted: boolean) =>
    updateTask.mutate({ path: { id: task.id }, body: { is_completed: isCompleted } });

  const deletingCount = tasks.filter((t) => t.list_id === deleting?.id).length;
  const deleteDescription =
    deletingCount === 0
      ? "You can restore it from the trash."
      : `This also deletes its ${deletingCount} ${deletingCount === 1 ? "task" : "tasks"}, including completed ones. You can restore them from the trash.`;

  const cardProps = (task: TaskResponse) => ({
    task,
    workspaceId,
    projectId,
    editable: canWrite,
    onToggleCompleted: toggleCompleted,
  });

  return (
    <DndContext
      sensors={sensors}
      collisionDetection={closestCorners}
      onDragStart={onDragStart}
      onDragEnd={onDragEnd}
      onDragCancel={() => setDragging(null)}
    >
      <div className="flex snap-x snap-mandatory scroll-px-4 items-start gap-4 overflow-x-auto p-4 sm:snap-none sm:p-6">
        {lists?.map((list) => {
          const open = openIn(list.id);
          return (
            <BoardColumn
              key={list.id}
              id={list.id}
              label={list.name}
              droppable={canWrite}
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
              <SortableContext items={open.map((t) => t.id)} strategy={verticalListSortingStrategy}>
                {open.map((task) => (
                  <SortableTaskCard key={task.id} columnId={list.id} {...cardProps(task)} />
                ))}
              </SortableContext>
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
          id="done"
          label="Done"
          droppable={canWrite}
          header={
            <h2 className="label-mono flex min-w-0 flex-1 justify-between gap-2">
              <span>Done</span>
              <span className="text-muted-foreground">{completed.length}</span>
            </h2>
          }
        >
          <SortableContext
            items={visibleDone.map((t) => t.id)}
            strategy={verticalListSortingStrategy}
          >
            {visibleDone.map((task) => (
              <SortableTaskCard key={task.id} columnId="done" {...cardProps(task)} />
            ))}
          </SortableContext>
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
      <DragOverlay>{dragging ? <TaskCardOverlay {...cardProps(dragging)} /> : null}</DragOverlay>
    </DndContext>
  );
}
