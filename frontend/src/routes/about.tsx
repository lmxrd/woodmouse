import { createFileRoute } from "@tanstack/react-router";

export const Route = createFileRoute("/about")({
  component: AboutPage,
});

function AboutPage() {
  return (
    <section className="space-y-4">
      <h1 className="text-3xl font-bold tracking-tight">About</h1>
      <p className="text-gray-600">
        Routes are files under <code className="rounded bg-gray-100 px-1 py-0.5">src/routes</code>,
        shared components live in{" "}
        <code className="rounded bg-gray-100 px-1 py-0.5">src/components</code>, and shared
        utilities in <code className="rounded bg-gray-100 px-1 py-0.5">src/lib</code>.
      </p>
    </section>
  );
}
