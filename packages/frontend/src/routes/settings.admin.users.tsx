import { createFileRoute } from "@tanstack/react-router";
import { useQuery } from "@tanstack/react-query";
import { meOptions } from "woodmouse-sdk/react-query";

import { UsersTab } from "../components/admin/users-tab";

export const Route = createFileRoute("/settings/admin/users")({
  component: () => {
    const { data: me } = useQuery(meOptions());
    return me?.super_admin ? <UsersTab meId={me.id} /> : <p>Not found.</p>;
  },
});
