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
import { getRouteApi } from "@tanstack/react-router";
import type { List, TaskResponse, UpdateTaskRequest } from "woodmouse-sdk";
import {
  createInboxTaskMutation,
  createListMutation,
  createTaskMutation,
  deleteListMutation,
  listListsOptions,
  listTasksOptions,
  updateListMutation,
  updateTaskMutation,
} from "woodmouse-sdk/react-query";

import { afterIdAt, provisionalPosition } from "../../lib/position";
import { taskFilterQuery } from "../../lib/projectSearch";
import { useWorkspaceRole } from "../../lib/useWorkspaceRole";
import { ActionMenu } from "../common/ActionMenu";
import { ConfirmDialog } from "../common/ConfirmDialog";
import { InlineAdd } from "../common/InlineAdd";
import { ProjectFilters } from "../views/ProjectFilters";
import { RenameInput } from "../common/RenameInput";
import { BoardColumn } from "./BoardColumn";
import { SortableTaskCard, TaskCardOverlay } from "./TaskCard";
import { Button } from "../ui/button";
import { isDone } from "../../lib/task";

const projectRoute = getRouteApi("/w/$workspaceId/p/$projectId");

/** The column id of tasks that are in no list: the project's inbox. */
const INBOX = "inbox";

/** How many completed tasks the Done column shows before "Show all". */
const DONE_LIMIT = 20;

/**
 * One column per list (open tasks only) plus a fixed Done column holding every completed task of the
 * project. Completion is a task state, not a list: dropping a card on Done completes it in place, and
 * dropping a completed card on a list reopens it and moves it there. Cards are ordered by their
 * `position`; dropping between two cards tells the server which card it now follows. The inbox column
 * holds the tasks that are in no list.
 */
export function Board({ projectId, workspaceId }: { projectId: string; workspaceId: string }) {
  const queryClient = useQueryClient();
  const { data: lists } = useQuery(listListsOptions({ path: { project_id: projectId } }));
  // The board shows every list and the Done column, so only the assignee and label filters apply.
  const search = projectRoute.useSearch();
  const { assignee_id, label_id, sprint_id, no_sprint } = taskFilterQuery(search, "board");
  const tasksQuery = listTasksOptions({
    path: { workspace_id: workspaceId },
    query: { project_id: projectId, assignee_id, label_id, sprint_id, no_sprint },
  });
  const { data: allTasks } = useQuery(tasksQuery);

  const refresh = () => queryClient.invalidateQueries();
  const createList = useMutation(createListMutation());
  const createTask = useMutation(createTaskMutation());
  const createInboxTask = useMutation(createInboxTaskMutation());
  const renameList = useMutation(updateListMutation());
  const deleteList = useMutation(deleteListMutation());
  // Moves update the cache first so the card stays where it was dropped instead of jumping back.
  const updateTask = useMutation({
    ...updateTaskMutation(),
    onMutate: async ({ path, body }) => {
      await queryClient.cancelQueries({ queryKey: tasksQuery.queryKey });
      const previous = queryClient.getQueryData<TaskResponse[]>(tasksQuery.queryKey);
      queryClient.setQueryData<TaskResponse[]>(tasksQuery.queryKey, (old) =>
        old?.map((t) => {
          if (t.id !== path.id) return t;
          const listId = body.list_id !== undefined ? body.list_id : t.list_id;
          const position =
            body.after_id !== undefined
              ? provisionalPosition(
                  old
                    .filter((o) => o.id !== t.id && (o.list_id ?? null) === (listId ?? null))
                    .filter((o) => !isDone(o))
                    .sort((a, b) => a.position - b.position),
                  body.after_id,
                )
              : t.position;
          return {
            ...t,
            list_id: listId,
            position,
            ...(body.is_completed != null
              ? { completed_at: body.is_completed ? new Date().toISOString() : null }
              : {}),
          };
        }),
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
  const openIn = (columnId: string) =>
    tasks
      .filter((t) => (t.list_id ?? INBOX) === columnId && !isDone(t))
      .sort((a, b) => a.position - b.position);
  const completed = tasks
    .filter((t) => isDone(t))
    .sort((a, b) => (b.completed_at ?? "").localeCompare(a.completed_at ?? ""));
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
        ? isDone(overTask)
          ? "done"
          : (overTask.list_id ?? INBOX)
        : undefined;
    if (!column) return;

    if (column === "done") {
      if (!isDone(task)) {
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
      (task.list_id ?? INBOX) === column &&
      !isDone(task) &&
      openIn(column).findIndex((t) => t.id === task.id) === index;
    if (sameSpot) return;

    const body: UpdateTaskRequest = { after_id: afterIdAt(siblings, index) };
    if ((task.list_id ?? INBOX) !== column) body.list_id = column === INBOX ? null : column;
    if (isDone(task)) body.is_completed = false;
    updateTask.mutate({ path: { id: task.id }, body });
  };

  const toggleCompleted = (task: TaskResponse, isCompleted: boolean) =>
    updateTask.mutate({ path: { id: task.id }, body: { is_completed: isCompleted } });

  const deletingCount = tasks.filter((t) => deleting && t.list_id === deleting.id).length;
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
      <ProjectFilters
        workspaceId={workspaceId}
        projectId={projectId}
        view="board"
        fields={["assignee", "label", "sprint"]}
      />
      <div className="flex snap-x snap-mandatory scroll-px-4 items-start gap-4 overflow-x-auto p-4 sm:snap-none sm:p-6">
        <BoardColumn
          id={INBOX}
          label="Inbox"
          droppable={canWrite}
          header={
            <h2 className="label-mono flex min-w-0 flex-1 justify-between gap-2">
              <span>Inbox</span>
              <span className="text-muted-foreground">{openIn(INBOX).length}</span>
            </h2>
          }
        >
          <SortableContext
            items={openIn(INBOX).map((t) => t.id)}
            strategy={verticalListSortingStrategy}
          >
            {openIn(INBOX).map((task) => (
              <SortableTaskCard key={task.id} columnId={INBOX} {...cardProps(task)} />
            ))}
          </SortableContext>
          {canWrite ? (
            <InlineAdd
              label="Add task"
              placeholder="Task title"
              onSubmit={(title) =>
                createInboxTask.mutate({ path: { project_id: projectId }, body: { title } })
              }
            />
          ) : null}
        </BoardColumn>

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
