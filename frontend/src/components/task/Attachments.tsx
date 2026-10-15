import { useRef, useState } from "react";
import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import type { FileView } from "woodmouse-client";
import {
  attachFileMutation,
  detachFileMutation,
  listAttachmentsOptions,
  listFilesOptions,
} from "woodmouse-client/react-query";

import { fileContentUrl } from "../../lib/api";
import { formatBytes } from "../../lib/upload";
import { userLabel } from "../../lib/user";
import { cn } from "../../lib/utils";
import { useUploader } from "../files/useUploader";
import { Button } from "../ui/button";
import { Popover, PopoverContent, PopoverTrigger } from "../ui/popover";

/** Files attached to a task: upload (or drop) new ones, attach from the library, or detach. */
export function Attachments({
  taskId,
  workspaceId,
  editable,
}: {
  taskId: string;
  workspaceId: string;
  editable: boolean;
}) {
  const queryClient = useQueryClient();
  const { data: attachments } = useQuery(listAttachmentsOptions({ path: { id: taskId } }));
  const refresh = () => queryClient.invalidateQueries();
  const attach = useMutation({ ...attachFileMutation(), onSuccess: refresh });
  const detach = useMutation({ ...detachFileMutation(), onSuccess: refresh });
  const { uploadFiles, isUploading, dialog } = useUploader(workspaceId);
  const input = useRef<HTMLInputElement>(null);
  const [over, setOver] = useState(false);
  const [libraryOpen, setLibraryOpen] = useState(false);
  const { data: library } = useQuery({
    ...listFilesOptions({ path: { id: workspaceId } }),
    enabled: libraryOpen,
  });

  if ((attachments?.length ?? 0) === 0 && !editable) return null;
  const attachedIds = new Set(attachments?.map((f) => f.id));
  const available = library?.filter((f) => !attachedIds.has(f.id)) ?? [];

  return (
    <section
      aria-label="Attachments"
      className={cn("flex flex-col gap-2", over && "bg-accent/50")}
      onDragOver={(e) => {
        if (!editable || !e.dataTransfer.types.includes("Files")) return;
        e.preventDefault();
        setOver(true);
      }}
      onDragLeave={() => setOver(false)}
      onDrop={(e) => {
        setOver(false);
        if (!editable || e.dataTransfer.files.length === 0) return;
        e.preventDefault();
        void uploadFiles([...e.dataTransfer.files], taskId);
      }}
    >
      <h2 className="label-mono text-muted-foreground">
        Attachments{attachments && attachments.length > 0 ? ` · ${attachments.length}` : ""}
      </h2>
      <div className="flex flex-wrap gap-2">
        {attachments?.map((file) => (
          <AttachmentChip
            key={file.id}
            file={file}
            onRemove={
              editable ? () => detach.mutate({ path: { id: taskId, file_id: file.id } }) : undefined
            }
          />
        ))}
      </div>
      {editable ? (
        <div className="flex flex-wrap items-center gap-2">
          <input
            ref={input}
            type="file"
            multiple
            className="sr-only"
            aria-label="Upload files"
            onChange={(e) => {
              void uploadFiles([...(e.target.files ?? [])], taskId);
              e.target.value = "";
            }}
          />
          <Button
            type="button"
            variant="outline"
            size="sm"
            disabled={isUploading}
            onClick={() => input.current?.click()}
          >
            {isUploading ? "Uploading…" : "Upload"}
          </Button>
          <Popover open={libraryOpen} onOpenChange={setLibraryOpen}>
            <PopoverTrigger asChild>
              <Button type="button" variant="ghost" size="sm" className="font-normal">
                Attach from library…
              </Button>
            </PopoverTrigger>
            <PopoverContent align="start" className="max-h-72 w-80 overflow-y-auto p-1">
              {available.length === 0 ? (
                <p className="p-3 text-sm text-muted-foreground">No other files in the library.</p>
              ) : (
                available.map((file) => (
                  <button
                    key={file.id}
                    type="button"
                    className="flex w-full items-center justify-between gap-3 px-2 py-1.5 text-left text-sm hover:bg-accent"
                    onClick={() => {
                      attach.mutate({ path: { id: taskId }, body: { file_id: file.id } });
                      setLibraryOpen(false);
                    }}
                  >
                    <span className="truncate">{file.name}</span>
                    <span className="label-mono shrink-0 text-muted-foreground">
                      {formatBytes(file.size)}
                    </span>
                  </button>
                ))
              )}
            </PopoverContent>
          </Popover>
          <span className="text-xs text-muted-foreground">or drop files here</span>
        </div>
      ) : null}
      {dialog}
    </section>
  );
}

function AttachmentChip({ file, onRemove }: { file: FileView; onRemove?: () => void }) {
  const extension = file.name.includes(".") ? file.name.split(".").pop()!.slice(0, 4) : "file";
  return (
    <div className="flex items-center gap-2 border border-border bg-card p-1.5">
      <a
        href={fileContentUrl(file.id)}
        target="_blank"
        rel="noreferrer"
        className="flex min-w-0 items-center gap-2 hover:underline"
        download={file.previewable ? undefined : file.name}
      >
        {file.previewable ? (
          <img
            src={fileContentUrl(file.id)}
            alt=""
            className="size-9 shrink-0 object-cover"
            loading="lazy"
          />
        ) : (
          <span className="label-mono grid size-9 shrink-0 place-items-center bg-accent text-[10px] text-accent-foreground">
            {extension}
          </span>
        )}
        <span className="min-w-0">
          <span className="block max-w-44 truncate text-sm">{file.name}</span>
          <span className="label-mono block text-muted-foreground">
            {formatBytes(file.size)}
            {file.uploaded_by ? ` · ${userLabel(file.uploaded_by)}` : ""}
          </span>
        </span>
      </a>
      {onRemove ? (
        <Button
          type="button"
          variant="ghost"
          size="icon-xs"
          aria-label={`Detach ${file.name}`}
          onClick={onRemove}
        >
          ✕
        </Button>
      ) : null}
    </div>
  );
}
