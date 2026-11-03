import { describe, expect, it } from "vitest";

import { ssoErrorMessage } from "./sso";

describe("ssoErrorMessage", () => {
  it("has a message for each code the backend sends", () => {
    for (const code of [
      "unavailable",
      "misconfigured",
      "state",
      "denied",
      "failed",
      "email",
      "unknown",
      "linked",
      "disabled",
      "setup",
    ]) {
      expect(ssoErrorMessage(code)).not.toMatch(/did not work/);
    }
  });

  it("falls back for an unknown code and is silent without one", () => {
    for (const code of ["<script>", "__proto__", "constructor", "toString"]) {
      expect(ssoErrorMessage(code)).toBe("Single sign-on did not work. Try again.");
    }
    expect(ssoErrorMessage(undefined)).toBeNull();
    expect(ssoErrorMessage("")).toBeNull();
  });
});
