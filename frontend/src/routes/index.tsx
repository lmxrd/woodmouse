import { createFileRoute } from "@tanstack/react-router";
import { useQuery } from "@tanstack/react-query";

interface DemoData {
  message: string;
}

export const Route = createFileRoute("/")({
  component: HomePage,
});

function HomePage() {
  // Demo query: replace the queryFn with a real fetch, e.g.
  //   queryFn: async () => (await fetch('/api/health')).json()
  const { data, isPending, isError } = useQuery<DemoData>({
    queryKey: ["demo"],
    queryFn: async () => {
      await new Promise((resolve) => setTimeout(resolve, 300));
      return { message: "Data fetched with TanStack Query." };
    },
  });

  return (
    <section className="space-y-6">
      <div>
        <h1 className="text-3xl font-bold tracking-tight">woodmouse</h1>
        <p className="mt-2 text-gray-600">
          React + Vite + Tailwind CSS + TanStack Router + TanStack Query.
        </p>
      </div>

      <div className="rounded-lg border border-gray-200 bg-white p-6 shadow-sm">
        {isPending && <p className="text-gray-500">Loading...</p>}
        {isError && <p className="text-red-600">Something went wrong.</p>}
        {data && <p data-testid="demo-message">{data.message}</p>}
      </div>
    </section>
  );
}
