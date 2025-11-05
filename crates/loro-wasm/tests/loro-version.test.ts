import { describe, expect, it } from "vitest";
import { LORO_VERSION } from "../bundler/index";

async function readPackageVersion(): Promise<string> {
  const module = await import("../package.json", {
    assert: { type: "json" },
  });
  return (module.default as { version: string }).version;
}

describe("LORO_VERSION", () => {
  it("matches package.json version", async () => {
    const packageVersion = await readPackageVersion();
    expect(LORO_VERSION()).toBe(packageVersion);
  });
});
