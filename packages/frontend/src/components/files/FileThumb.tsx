import type { FileView } from "woodmouse-client";

import { fileContentUrl } from "../../lib/api";

/** A small preview of a file: the image itself, or its extension for anything else. */
export function FileThumb({ file }: { file: FileView }) {
  const extension = file.name.includes(".") ? file.name.split(".").pop()!.slice(0, 4) : "file";
  return file.previewable ? (
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
  );
}
