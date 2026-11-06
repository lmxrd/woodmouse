import { expect, test, type Page } from "@playwright/test";

import { Authenticator } from "./totp";

// One site, set up by the first test and used by the next ones (the server starts with an empty database),
// so the tests run in order, in one worker that keeps this module's state.
test.describe.configure({ mode: "serial" });

const EMAIL = "admin@e2e.test";
const PASSWORD = "an e2e password";
// Set up by the two-factor test; a later sign-in may wait up to 30 s for a fresh code.
let app: Authenticator | undefined;

async function signIn(page: Page) {
  await page.goto("/login");
  await page.getByRole("textbox", { name: "Email" }).fill(EMAIL);
  await page.getByRole("textbox", { name: "Password" }).fill(PASSWORD);
  await page.getByRole("button", { name: "Log in" }).click();
  if (app) {
    await expect(page).toHaveURL(/step=totp/);
    await page.getByRole("textbox", { name: "Code" }).fill(await app.code());
    await page.getByRole("button", { name: "Verify" }).click();
  }
  await expect(page).toHaveURL(/\/w\/[0-9a-f-]+/);
}

async function signOut(page: Page) {
  await page.goto("/");
  await page.getByRole("button", { name: "Log out" }).click();
  await expect(page).toHaveURL(/\/login/);
}

test("first setup makes the super admin and a workspace", async ({ page }) => {
  await page.goto("/");
  await expect(page).toHaveURL(/\/login/);
  await expect(page.getByRole("heading", { name: "Set up Woodmouse" })).toBeVisible();
  await page.getByRole("textbox", { name: "Your name (optional)" }).fill("Admin");
  await page.getByRole("textbox", { name: "Email" }).fill(EMAIL);
  await page.getByRole("textbox", { name: "Password" }).fill(PASSWORD);
  await page.getByRole("button", { name: "Create admin account" }).click();

  await expect(page.getByRole("heading", { name: "Start your first workspace" })).toBeVisible();
  await page.getByRole("button", { name: "Create workspace" }).click();
  await page.getByRole("textbox", { name: "Workspace name" }).fill("Home");
  await page.keyboard.press("Enter");
  await expect(page).toHaveURL(/\/w\/[0-9a-f-]+/);
  await expect(page.getByRole("button", { name: "Log out" })).toBeVisible();
});

test("signing out and in again", async ({ page }) => {
  await signIn(page);
  await signOut(page);
  // Signed out for good: the app sends the visitor back to the login page.
  await page.goto("/settings/account");
  await expect(page).toHaveURL(/\/login/);
  await signIn(page);
});

test("two-factor sign-in with a code from an authenticator app", async ({ page }) => {
  test.setTimeout(90_000);
  await signIn(page);
  await page.goto("/settings/password");
  await page.getByRole("button", { name: "Set up two-factor sign-in" }).click();
  const secret = await page.getByRole("textbox", { name: "Key" }).inputValue();
  expect(secret).toMatch(/^[A-Z2-7]{32}$/);
  const authenticator = new Authenticator(secret);
  await page.getByRole("textbox", { name: "Code from the app" }).fill(await authenticator.code());
  await page.getByRole("button", { name: "Turn on" }).click();
  await expect(page.getByText("Keep these recovery codes somewhere safe")).toBeVisible();
  await page.getByRole("button", { name: "I have saved them" }).click();
  await expect(page.getByText("On. Signing in also asks for a code")).toBeVisible();
  app = authenticator;

  // From now on the password alone is not enough.
  await signOut(page);
  await signIn(page);
  await expect(page.getByRole("button", { name: "Log out" })).toBeVisible();
});

test("a project and a task", async ({ page }) => {
  test.setTimeout(90_000);
  await signIn(page);

  await page.getByRole("button", { name: "New project" }).click();
  await page.keyboard.type("House");
  await page.keyboard.press("Enter");
  await expect(page).toHaveURL(/\/p\/[0-9a-f-]+/);
  await expect(page.getByRole("link", { name: "House" })).toBeVisible();

  await page.getByRole("button", { name: "Add task" }).first().click();
  await page.getByRole("textbox", { name: "Task title" }).fill("Paint the hall");
  await page.keyboard.press("Enter");
  await expect(page.getByRole("link", { name: "Paint the hall" })).toBeVisible();

  // It is saved: still there after a reload.
  await page.reload();
  await expect(page.getByRole("link", { name: "Paint the hall" })).toBeVisible();
});
