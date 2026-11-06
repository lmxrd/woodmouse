# woodmouse-e2e

End-to-end smoke tests: Playwright drives Chromium through the built image,
on a fresh database. They cover the flows that must never break: first
setup, signing out and in, two-factor authentication, and a project with a
task. Everything else is tested closer to the code.

```sh
mise run test:e2e
```

builds the image, starts it with an empty volume on `localhost:8180`
(`E2E_PORT`), runs the tests and removes the container, printing its log if
a test failed (`scripts/e2e.sh`; podman when installed, else Docker, or
`ENGINE=docker`). CI runs them against the image it builds and keeps
Playwright's HTML report of a failed run.
