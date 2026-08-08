import { describe, expect, it } from "vitest";
import { LoroDoc } from "../bundler/index";

// This file intentionally traps the WASM instance once (JSONPath `match()` is
// registered in the parser but its evaluator is `unimplemented!()`). Keep it as
// the only trap-triggering test in this file: after a trap the instance must be
// considered corrupt. Other test files get fresh module instances.
describe("panic hook global slot", () => {
  it("captures the full panic message and the complete stack on globalThis", () => {
    const doc = new LoroDoc();
    doc.getMap("map").set("a", "hello");

    let caught: unknown;
    try {
      doc.JSONPath(`$[?match(@.a, "h.*")]`);
    } catch (e) {
      caught = e;
    }

    // The trap itself is still an engine-made RuntimeError without details...
    expect(caught).toBeInstanceOf(WebAssembly.RuntimeError);
    expect(String((caught as Error).message)).not.toContain("match()");

    // ...but the global slot carries the full panic info.
    const last = (globalThis as any).__LORO_WASM_LAST_PANIC__;
    expect(last).toBeDefined();
    expect(String(last.message)).toContain(
      "JSONPath function `match()` is declared but not implemented",
    );
    expect(String(last.message)).toContain("jsonpath_impl.rs");

    // The stack must include the actual Rust call path that led to the panic,
    // not just the panic-hook machinery: V8's default 10-frame limit truncates
    // everything below `panic_fmt`, which is what console_error_panic_hook
    // used to print.
    const stack = String(last.stack);
    expect(stack).toContain("jsonpath");
    // The JS -> wasm entry frame is visible as well.
    expect(stack).toMatch(/loro_wasm|index\.js/);
    // Sanity: the stack is deep, not the default 10 frames.
    expect(stack.split("\n").length).toBeGreaterThan(10);
  });
});
