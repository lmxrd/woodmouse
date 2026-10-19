import { useEffect, useRef } from "react";
import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import { meOptions, meQueryKey, updateMeMutation } from "woodmouse-sdk/react-query";

/** The browser's timezone name (`Europe/Oslo`), or undefined when it cannot tell. */
export function browserTimezone(): string | undefined {
  try {
    return Intl.DateTimeFormat().resolvedOptions().timeZone || undefined;
  } catch {
    return undefined;
  }
}

/** The first time someone signs in, remember their browser's timezone on their account (they can change it). */
export function useDetectTimezone() {
  const queryClient = useQueryClient();
  // Only reads what the app already loaded for the signed-in user; never asks (this also runs on the login page).
  const { data: me } = useQuery({ ...meOptions(), enabled: false });
  const update = useMutation({
    ...updateMeMutation(),
    meta: { inline: true },
    onSuccess: (user) => queryClient.setQueryData(meQueryKey(), user),
  });
  const tried = useRef(false);
  useEffect(() => {
    const timezone = browserTimezone();
    if (me && !me.timezone && timezone && !tried.current) {
      tried.current = true;
      update.mutate({ body: { timezone } });
    }
  }, [me, update]);
}
