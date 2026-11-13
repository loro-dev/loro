import { strict as assert } from "node:assert";
import { syncRustVersionFile } from "./cargo-release.ts";

function withVersionFile(contents: string, check: (path: string) => void) {
  const dir = Deno.makeTempDirSync();
  const path = `${dir}/VERSION`;
  try {
    Deno.writeTextFileSync(path, contents);
    check(path);
  } finally {
    Deno.removeSync(dir, { recursive: true });
  }
}

Deno.test("release preparation accepts an already synchronized version unchanged", () => {
  withVersionFile("1.18.0\n", (path) => {
    syncRustVersionFile("1.18.0", path);
    syncRustVersionFile("1.18.0", path);
    assert.equal(Deno.readTextFileSync(path), "1.18.0\n");
  });
});

Deno.test("release preparation advances the version", () => {
  withVersionFile("1.17.1\n", (path) => {
    syncRustVersionFile("1.18.0", path);
    assert.equal(Deno.readTextFileSync(path), "1.18.0");
  });
});

Deno.test("release preparation rejects a downgrade without modifying the version", () => {
  withVersionFile("1.18.0\n", (path) => {
    assert.throws(() => syncRustVersionFile("1.17.1", path), /is lower/);
    assert.equal(Deno.readTextFileSync(path), "1.18.0\n");
  });
});

Deno.test("release preparation rejects an invalid version file unchanged", () => {
  withVersionFile("broken\n", (path) => {
    assert.throws(() => syncRustVersionFile("1.18.0", path), /Invalid version/);
    assert.equal(Deno.readTextFileSync(path), "broken\n");
  });
});
