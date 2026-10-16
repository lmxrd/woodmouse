import { useState, type ReactNode } from "react";
import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import { toast } from "sonner";
import type { FileView } from "woodmouse-client";
import {
  attachFileMutation,
  attachPageFileMutation,
  getConfigOptions,
} from "woodmouse-client/react-query";

import { formatBytes, uploadFile } from "../../lib/upload";
import { DuplicateFileDialog } from "./DuplicateFileDialog";

interface PendingDuplicate {
  name: string;
  existing: FileView;
  taskId?: string;
  pageId?: string;
}

/**
 * Uploads files into a workspace's library (and onto a task or wiki page when `taskId` or `pageId` is given), checking the
 * size limit first and asking what to do about duplicates. Render `dialog` somewhere in the tree.
 */
export function useUploader(workspaceId: string): {
  uploadFiles: (files: File[], taskId?: string, pageId?: string) => Promise<void>;
  isUploading: boolean;
  dialog: ReactNode;
} {
  const queryClient = useQueryClient();
  const { data: config } = useQuery(getConfigOptions());
  const attach = useMutation({
    ...attachFileMutation(),
    onSuccess: () => queryClient.invalidateQueries(),
  });
  const attachPage = useMutation({
    ...attachPageFileMutation(),
    onSuccess: () => queryClient.invalidateQueries(),
  });
  const [uploading, setUploading] = useState(0);
  const [duplicates, setDuplicates] = useState<PendingDuplicate[]>([]);

  const uploadFiles = async (files: File[], taskId?: string, pageId?: string) => {
    for (const file of files) {
      if (config && file.size > config.max_upload_bytes) {
        toast.error(`${file.name} is larger than ${formatBytes(config.max_upload_bytes)}.`);
        continue;
      }
      setUploading((n) => n + 1);
      const result = await uploadFile(workspaceId, file, taskId);
      setUploading((n) => n - 1);
      if (result.status === "created") {
        if (pageId) {
          await attachPage.mutateAsync({ path: { id: pageId }, body: { file_id: result.file.id } });
        }
        await queryClient.invalidateQueries();
      } else if (result.status === "duplicate") {
        setDuplicates((list) => [
          ...list,
          { name: file.name, existing: result.existing, taskId, pageId },
        ]);
      } else if (result.status === "too_large") {
        toast.error(`${file.name} is larger than the upload limit.`);
      } else {
        toast.error(`Couldn't upload ${file.name}. Try again.`);
      }
    }
  };

  const current = duplicates[0];
  const resolve = () => setDuplicates((list) => list.slice(1));

  return {
    uploadFiles,
    isUploading: uploading > 0,
    dialog: current ? (
      <DuplicateFileDialog
        key={current.name + current.existing.id}
        uploadedName={current.name}
        existing={current.existing}
        attachesToTask={!!current.taskId || !!current.pageId}
        onCancel={resolve}
        onUseExisting={() => {
          if (current.taskId) {
            attach.mutate({ path: { id: current.taskId }, body: { file_id: current.existing.id } });
          } else if (current.pageId) {
            attachPage.mutate({
              path: { id: current.pageId },
              body: { file_id: current.existing.id },
            });
          }
          resolve();
        }}
      />
    ) : null,
  };
}
