import { useQuery } from "@tanstack/react-query";
import { meOptions } from "woodmouse-client/react-query";

import type { WeekStart } from "./calendar";
import {
  DEFAULT_PREFS,
  formatDate,
  formatDateShort,
  formatDateTime,
  formatWhen,
  type DisplayPrefs,
} from "./format";

/** The date and time formatters bound to the signed-in user's preferences. */
export function useFormatters() {
  const { data: me } = useQuery(meOptions());
  const prefs: DisplayPrefs = me
    ? { date_format: me.date_format, time_format: me.time_format }
    : DEFAULT_PREFS;
  const weekStart: WeekStart = me?.week_start === "sunday" ? "sunday" : "monday";
  return {
    prefs,
    weekStart,
    date: (epoch: number) => formatDate(epoch, prefs),
    dateShort: (epoch: number) => formatDateShort(epoch, prefs),
    dateTime: (epoch: number) => formatDateTime(epoch, prefs),
    when: (epoch: number) => formatWhen(epoch, prefs),
  };
}
