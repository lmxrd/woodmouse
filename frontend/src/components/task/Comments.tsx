import { useState } from "react";
import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import type { CommentView } from "woodmouse-client";
import {
  addCommentMutation,
  deleteCommentMutation,
  listCommentsOptions,
  meOptions,
  updateCommentMutation,
} from "woodmouse-client/react-query";

import { useFormatters } from "../../lib/useFormatters";
import { userLabel } from "../../lib/user";
import { Markdown } from "../Markdown";
import { MarkdownEditor } from "../MarkdownEditor";
import { UserAvatar } from "../UserAvatar";
import { Button } from "../ui/button";

/** The comments of a task (markdown), with a composer. Authors edit; authors and admins delete. */
export function Comments({
  taskId,
  editable,
  isAdmin,
}: {
  taskId: string;
  editable: boolean;
  isAdmin: boolean;
}) {
  const queryClient = useQueryClient();
  const { when, dateTime } = useFormatters();
  const { data: me } = useQuery(meOptions());
  const { data: comments } = useQuery(listCommentsOptions({ path: { id: taskId } }));
  const refresh = () => queryClient.invalidateQueries();
  const add = useMutation({ ...addCommentMutation(), onSuccess: refresh });
  const update = useMutation({ ...updateCommentMutation(), onSuccess: refresh });
  const remove = useMutation({ ...deleteCommentMutation(), onSuccess: refresh });
  const [draft, setDraft] = useState("");
  const [editing, setEditing] = useState<{ id: string; body: string } | null>(null);

  const submit = () => {
    if (!draft.trim()) return;
    add.mutate({ path: { id: taskId }, body: { body: draft } });
    setDraft("");
  };

  const render = (comment: CommentView) => {
    const own = comment.author?.id === me?.id;
    const author = comment.author ? userLabel(comment.author) : "Someone";
    return (
      <li key={comment.id} className="flex gap-3 py-2">
        <UserAvatar name={author} size="md" className="mt-0.5" />
        <div className="min-w-0 flex-1">
          <p className="flex flex-wrap items-baseline gap-x-2 text-sm">
            <b className="font-medium">{author}</b>
            <span className="label-mono text-muted-foreground" title={dateTime(comment.created_at)}>
              {when(comment.created_at)}
            </span>
            {comment.edited_at ? (
              <span
                className="label-mono text-muted-foreground"
                title={dateTime(comment.edited_at)}
              >
                edited
              </span>
            ) : null}
            {editable && (own || isAdmin) && editing?.id !== comment.id ? (
              <span className="ml-auto flex gap-1">
                {own ? (
                  <Button
                    type="button"
                    variant="ghost"
                    size="xs"
                    onClick={() => setEditing({ id: comment.id, body: comment.body })}
                  >
                    Edit
                  </Button>
                ) : null}
                <Button
                  type="button"
                  variant="ghost"
                  size="xs"
                  className="text-destructive"
                  onClick={() => remove.mutate({ path: { id: comment.id } })}
                >
                  Delete
                </Button>
              </span>
            ) : null}
          </p>
          {editing?.id === comment.id ? (
            <div className="mt-1 flex flex-col gap-2">
              <MarkdownEditor
                id={`edit-${comment.id}`}
                value={editing.body}
                rows={3}
                onChange={(body) => setEditing({ id: comment.id, body })}
              />
              <div className="flex gap-2">
                <Button
                  type="button"
                  size="sm"
                  disabled={!editing.body.trim()}
                  onClick={() => {
                    update.mutate({ path: { id: comment.id }, body: { body: editing.body } });
                    setEditing(null);
                  }}
                >
                  Save
                </Button>
                <Button type="button" variant="ghost" size="sm" onClick={() => setEditing(null)}>
                  Cancel
                </Button>
              </div>
            </div>
          ) : (
            <Markdown className="mt-0.5">{comment.body}</Markdown>
          )}
        </div>
      </li>
    );
  };

  return (
    <div className="flex flex-col gap-3">
      {comments && comments.length === 0 ? (
        <p className="text-sm text-muted-foreground">No comments yet.</p>
      ) : null}
      <ul className="divide-y divide-border">{comments?.map(render)}</ul>
      {editable ? (
        <div className="flex flex-col gap-2">
          <MarkdownEditor
            id="new-comment"
            value={draft}
            rows={3}
            placeholder="Write a comment… (Ctrl+Enter to send)"
            onChange={setDraft}
            onSubmit={submit}
          />
          <div>
            <Button
              type="button"
              size="sm"
              disabled={!draft.trim() || add.isPending}
              onClick={submit}
            >
              Comment
            </Button>
          </div>
        </div>
      ) : null}
    </div>
  );
}
