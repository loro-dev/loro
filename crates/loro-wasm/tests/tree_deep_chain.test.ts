import { spawnSync } from "node:child_process";
import { fileURLToPath } from "node:url";
import { expect, it } from "vitest";

it("reads a 4000-level snapshot and its full hierarchy without trapping", () => {
  // A WASM stack trap poisons the instance. Keep it in a child process so a
  // regression cannot corrupt the instance used by other tests.
  const result = spawnSync(
    process.execPath,
    [
      "-e",
      `
      const assert = require("node:assert/strict");
      const { LoroDoc } = require(process.argv[1]);
      const source = new LoroDoc();
      source.setPeerId(1);
      const tree = source.getTree("t");
      let node = tree.createNode();
      const ids = [node.id];
      for (let depth = 1; depth < 4000; depth++) {
        node = tree.createNode(node.id);
        ids.push(node.id);
      }
      node.data.set("leaf", true);
      const snapshot = source.export({ mode: "snapshot" });
      source.free();
      for (const read of ["roots", "getNodes", "toArray", "toJSON", "nodeJSON"]) {
        const doc = new LoroDoc();
        doc.import(snapshot);
        const imported = doc.getTree("t");
        if (read === "roots") {
          assert.equal(imported.roots().length, 1);
        } else if (read === "getNodes") {
          assert.equal(imported.getNodes({ withDeleted: false }).length, 4000);
        } else {
          let nodes = read === "nodeJSON"
            ? [imported.roots()[0].toJSON()]
            : imported[read]();
          for (let depth = 0; depth < 4000; depth++) {
            assert.equal(nodes.length, 1);
            assert.equal(nodes[0].id, ids[depth]);
            assert.equal(nodes[0].index, 0);
            if (depth === 3999) {
              const meta = read === "toArray" ? nodes[0].meta.toJSON() : nodes[0].meta;
              assert.equal(meta.leaf, true);
            }
            nodes = nodes[0].children;
          }
          assert.equal(nodes.length, 0);
        }
        doc.free();
      }
    `,
      fileURLToPath(new URL("../nodejs/index.js", import.meta.url)),
    ],
    { encoding: "utf8", timeout: 30_000 },
  );
  expect(result.error, result.stderr).toBeUndefined();
  expect(result.status, result.stderr).toBe(0);
});
