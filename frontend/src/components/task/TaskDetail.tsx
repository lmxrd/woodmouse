import { useState, type ReactNode } from "react";
import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import type { Label, TaskResponse } from "woodmouse-client";
import {
  createLabelMutation,
  deleteTaskMutation,
  getListOptions,
  getTaskOptions,
  listCommentsOptions,
  listLabelsOptions,
  listListsOptions,
  listMembersOptions,
  updateTaskMutation,
} from "woodmouse-client/react-query";

import { useWorkspaceRole } from "../../lib/useWorkspaceRole";
import { Field } from "../Field";
import { MarkdownEditor } from "../MarkdownEditor";
import { SaveStatus } from "../SaveStatus";
import { MentionedIn } from "./MentionedIn";
import { TaskHistory } from "./TaskHistory";
import { Button } from "../ui/button";
import { Checkbox } from "../ui/checkbox";
import { Select, SelectContent, SelectItem, SelectTrigger, SelectValue } from "../ui/select";
import { Tabs, TabsContent, TabsList, TabsTrigger } from "../ui/tabs";
import { Attachments } from "./Attachments";
import { Comments } from "./Comments";
import { DateField } from "./DateField";
import { LabelPicker } from "./LabelPicker";
import { MemberPicker } from "./MemberPicker";
import { PrioritySelect } from "./PrioritySelect";
import { SubtaskTree } from "./SubtaskTree";
import { TaskActivity } from "./TaskActivity";
import { TaskLinks } from "./TaskLinks";
import { isDone } from "../../lib/task";

/** The Select value for "no list": the project's inbox. */
const INBOX = "inbox";

/**
 * Everything about one task, with autosave. Container-agnostic: the dialog route and the full-page
 * route both render it, passing their own `header` (breadcrumb, open-as-page, close), `onDeleted`
 * and `onOpenTask` (how to navigate to a related task or subtask).
 */
export function TaskDetail({
  taskId,
  workspaceId,
  header,
  onDeleted,
  onOpenTask,
}: {
  taskId: string;
  workspaceId: string;
  header: (context: { projectId: string | undefined; listName: string | undefined }) => ReactNode;
  onDeleted: () => void;
  onOpenTask: (taskId: string) => void;
}) {
  const { data: task, isError } = useQuery({
    ...getTaskOptions({ path: { id: taskId } }),
    retry: false,
  });
  const { data: list } = useQuery({
    ...getListOptions({ path: { id: task?.list_id ?? "" } }),
    enabled: !!task,
  });

  if (isError) return <p className="p-6 text-sm text-muted-foreground">This task doesn't exist.</p>;
  if (!task) return <p className="p-6 text-sm text-muted-foreground">Loading…</p>;

  return (
    <>
      {header({ projectId: list?.project_id, listName: list?.name })}
      <TaskForm
        key={task.id}
        task={task}
        workspaceId={workspaceId}
        projectId={list?.project_id}
        onDeleted={onDeleted}
        onOpenTask={onOpenTask}
      />
    </>
  );
}

function TaskForm({
  task,
  workspaceId,
  projectId,
  onDeleted,
  onOpenTask,
}: {
  task: TaskResponse;
  workspaceId: string;
  projectId: string | undefined;
  onDeleted: () => void;
  onOpenTask: (taskId: string) => void;
}) {
  const queryClient = useQueryClient();
  const refresh = () => queryClient.invalidateQueries();
  const { canWrite, isAdmin } = useWorkspaceRole(workspaceId);

  const { data: lists } = useQuery({
    ...listListsOptions({ path: { project_id: projectId ?? "" } }),
    enabled: !!projectId,
  });
  const { data: members } = useQuery(listMembersOptions({ path: { id: workspaceId } }));
  const { data: labels } = useQuery(listLabelsOptions({ path: { workspace_id: workspaceId } }));
  const { data: comments } = useQuery(listCommentsOptions({ path: { id: task.id } }));
  const { data: parent } = useQuery({
    ...getTaskOptions({ path: { id: task.parent_id ?? "" } }),
    enabled: !!task.parent_id,
  });

  const update = useMutation({
    ...updateTaskMutation(),
    meta: { inline: true },
    onSuccess: refresh,
  });
  const save = (body: Parameters<typeof update.mutate>[0]["body"]) =>
    update.mutate({ path: { id: task.id }, body });
  const createLabel = useMutation({ ...createLabelMutation(), onSuccess: refresh });
  const remove = useMutation({
    ...deleteTaskMutation(),
    onSuccess: () => {
      // Leave first: refetching the deleted task would only 404.
      onDeleted();
      void refresh();
    },
  });

  const [title, setTitle] = useState(task.title);
  const [description, setDescription] = useState(task.description);
  const [confirmDelete, setConfirmDelete] = useState(false);
  const [historyOpen, setHistoryOpen] = useState(false);

  return (
    <div className="grid sm:grid-cols-[minmax(0,1fr)_200px]">
      <div className="flex flex-col gap-5 p-5 sm:border-r sm:border-border">
        {parent ? (
          <button
            type="button"
            onClick={() => onOpenTask(parent.id)}
            className="label-mono self-start text-muted-foreground hover:text-foreground"
          >
            ← Subtask of {parent.title}
          </button>
        ) : null}
        <div className="flex items-start gap-3">
          <Checkbox
            className="mt-2.5 size-[18px]"
            aria-label="Completed"
            checked={isDone(task)}
            disabled={!canWrite}
            onCheckedChange={(checked) => save({ is_completed: checked === true })}
          />
          <textarea
            aria-label="Title"
            rows={1}
            value={title}
            readOnly={!canWrite}
            onChange={(e) => setTitle(e.target.value)}
            onKeyDown={(e) => {
              if (e.key === "Enter") {
                e.preventDefault();
                e.currentTarget.blur();
              }
            }}
            onBlur={() => {
              const trimmed = title.trim();
              if (!trimmed) return setTitle(task.title);
              if (trimmed !== task.title) save({ title: trimmed });
            }}
            className={`min-w-0 flex-1 resize-none border-b border-dashed border-transparent bg-transparent font-display text-2xl leading-tight font-medium field-sizing-content outline-none hover:border-border focus:border-ring ${isDone(task) ? "text-muted-foreground line-through" : ""}`}
          />
        </div>

        <Button
          type="button"
          variant="ghost"
          size="xs"
          className="-mb-3 self-start font-normal text-muted-foreground"
          onClick={() => setHistoryOpen(true)}
        >
          History of title and description
        </Button>
        <TaskHistory
          taskId={task.id}
          open={historyOpen}
          onOpenChange={setHistoryOpen}
          canRestore={canWrite}
          onRestored={(restored) => {
            setTitle(restored.title);
            setDescription(restored.description);
          }}
        />

        <Field label="Description" htmlFor="task-description">
          <MarkdownEditor
            id="task-description"
            value={description}
            readOnly={!canWrite}
            initialMode={task.description.trim() ? "preview" : "write"}
            onChange={setDescription}
            onBlur={() => description !== task.description && save({ description })}
          />
        </Field>

        <SubtaskTree task={task} editable={canWrite} onOpenTask={onOpenTask} />
        <MentionedIn taskId={task.id} workspaceId={workspaceId} />
        <TaskLinks
          taskId={task.id}
          workspaceId={workspaceId}
          editable={canWrite}
          onOpenTask={onOpenTask}
        />
        <Attachments taskId={task.id} workspaceId={workspaceId} editable={canWrite} />

        <Tabs defaultValue="comments">
          <TabsList>
            <TabsTrigger value="comments">Comments · {comments?.length ?? 0}</TabsTrigger>
            <TabsTrigger value="activity">Activity</TabsTrigger>
          </TabsList>
          <TabsContent value="comments" className="pt-2">
            <Comments taskId={task.id} editable={canWrite} isAdmin={isAdmin} />
          </TabsContent>
          <TabsContent value="activity" className="pt-2">
            <TaskActivity taskId={task.id} workspaceId={workspaceId} />
          </TabsContent>
        </Tabs>

        {canWrite ? (
          <div className="mt-auto flex items-center justify-between gap-3">
            {confirmDelete ? (
              <span className="flex items-center gap-2 text-sm">
                <Button
                  type="button"
                  variant="destructive"
                  size="sm"
                  disabled={remove.isPending}
                  onClick={() => remove.mutate({ path: { id: task.id } })}
                >
                  Delete task
                </Button>
                <Button
                  type="button"
                  variant="ghost"
                  size="sm"
                  onClick={() => setConfirmDelete(false)}
                >
                  Keep
                </Button>
                <span className="text-xs text-muted-foreground">
                  It can be restored from the trash.
                </span>
              </span>
            ) : (
              <Button
                type="button"
                variant="ghost"
                size="sm"
                className="text-destructive"
                onClick={() => setConfirmDelete(true)}
              >
                Delete task…
              </Button>
            )}
            <SaveStatus status={update.status} />
          </div>
        ) : null}
      </div>

      <fieldset
        disabled={!canWrite}
        className="m-0 flex min-w-0 flex-col gap-4 border-0 border-t border-border bg-muted/40 p-5 sm:border-t-0"
      >
        <Field label="List">
          <Select
            value={task.list_id ?? INBOX}
            onValueChange={(listId) => save({ list_id: listId === INBOX ? null : listId })}
          >
            <SelectTrigger aria-label="List" className="w-full">
              <SelectValue />
            </SelectTrigger>
            <SelectContent>
              <SelectItem value={INBOX}>Inbox</SelectItem>
              {lists?.map((l) => (
                <SelectItem key={l.id} value={l.id}>
                  {l.name}
                </SelectItem>
              ))}
            </SelectContent>
          </Select>
        </Field>
        <Field label="Priority">
          <PrioritySelect value={task.priority} onChange={(priority) => save({ priority })} />
        </Field>
        <Field label="Start" htmlFor="task-start">
          <DateField
            id="task-start"
            value={task.start_date}
            onChange={(v) => save({ start_date: v })}
          />
        </Field>
        <Field label="Due" htmlFor="task-due">
          <DateField id="task-due" value={task.due_date} onChange={(v) => save({ due_date: v })} />
        </Field>
        <Field label="Assignees">
          <MemberPicker
            members={members ?? []}
            selected={task.assignees}
            onChange={(ids) => save({ assignee_ids: ids })}
          />
        </Field>
        <Field label="Labels">
          <LabelPicker
            labels={labels ?? []}
            selected={task.labels}
            onChange={(ids) => save({ label_ids: ids })}
            onCreate={(name): Promise<Label> =>
              createLabel.mutateAsync({ path: { workspace_id: workspaceId }, body: { name } })
            }
          />
        </Field>
      </fieldset>
    </div>
  );
}
