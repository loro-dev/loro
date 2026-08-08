import { describe, expect, it } from "vitest";
import { spawnSync } from "node:child_process";
import path from "node:path";
import { fileURLToPath } from "node:url";

// OOM in WASM does not go through the panic hook (allocation failure traps
// directly), so this is verified in a child node process that caps the WASM
// memory at 64MB via V8's `--wasm-max-mem-pages` flag.
describe("out-of-memory reporting", () => {
  it("reports the failing allocation size before trapping", (ctx) => {
    const nodejsEntry = path.resolve(
      path.dirname(fileURLToPath(import.meta.url)),
      "../nodejs/index.js",
    );
    const script = `
      import { LoroDoc } from ${JSON.stringify(nodejsEntry)};
      const doc = new LoroDoc();
      const text = doc.getText("t");
      const chunk = "x".repeat(4 * 1024 * 1024);
      try {
        for (let i = 0; i < 200; i++) text.insert(text.length, chunk);
        console.log("NO_OOM");
      } catch (e) {
        console.log("CAUGHT:", e.constructor.name);
      }
      const last = globalThis.__LORO_WASM_LAST_PANIC__;
      console.log("SLOT:", last ? String(last.message) : "<none>");
      // Terminate right after printing: the trapped instance is poisoned, and
      // finalizing its documents later (e.g. via GC) can re-trap and take the
      // process down with a nonzero exit code.
      process.exit(0);
    `;
    // `--wasm-max-mem-pages` is a V8-internal flag; if a future Node/V8
    // renames or removes it, skip instead of failing mysteriously.
    const probe = spawnSync(process.execPath, ["--wasm-max-mem-pages=1024", "-e", ""], {
      encoding: "utf8",
    });
    if (probe.error || String(probe.stderr).includes("bad option")) {
      ctx.skip();
    }
    const res = spawnSync(
      process.execPath,
      ["--wasm-max-mem-pages=1024", "--input-type=module", "-e", script],
      { encoding: "utf8", stdio: ["ignore", "pipe", "pipe"] },
    );
    if (res.error) {
      throw res.error;
    }
    const out = res.stdout;
    // The instance still traps (OOM is unrecoverable)...
    expect(out).toContain("CAUGHT: RuntimeError");
    // ...but the global slot now says exactly what happened.
    expect(out).toContain(
      "SLOT: loro-crdt: out of WASM memory: allocation of",
    );
    expect(out).toContain("bytes failed");
    expect(out).not.toContain("NO_OOM");
    expect(out).not.toContain("SLOT: <none>");
  }, 60_000);
});
