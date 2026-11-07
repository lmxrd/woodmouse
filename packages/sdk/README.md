# woodmouse-sdk

The TypeScript client of Woodmouse's API, generated from the backend's OpenAPI
spec by [hey-api](https://heyapi.dev) (`openapi-ts.config.ts`): a fetch
client, TanStack Query options and mutations, and zod schemas that check
requests and responses. The frontend imports it as `woodmouse-sdk`.

`src/` is generated and never committed: `mise run gen:sdk` writes the spec,
then the client (every frontend task runs it first).
