import { describe, expect, it } from "vitest";
import { LoroDoc } from "../bundler/index";

// A root touched by history is visible even when its ops add up to an empty
// value, both for the peer that wrote them and for a replay of that history.
// See loro-dev/loro#1156.
describe("empty root containers", () => {
  it("a replay shows the same empty root as the peer", () => {
    const a = new LoroDoc();
    a.setPeerId(1);
    a.getText("t").insert(0, "ab");
    a.commit();
    a.getText("t").delete(0, 2);
    a.commit();
    expect(a.toJSON()).toEqual({ t: "" });

    const replay = new LoroDoc();
    replay.import(a.export({ mode: "update" }));
    expect(replay.toJSON()).toEqual({ t: "" });
  });
});
