import { describe, expect, expectTypeOf, it } from "vitest";
import { Loro } from "../src";
import { Container, LoroText, OpId } from "../src";
import { setDebug } from "loro-wasm";

it("#211", () => {
  const loro1 = new Loro();
  loro1.setPeerId(0n);
  const text1 = loro1.getText("text");

  const loro2 = new Loro();
  loro2.setPeerId(1n);
  const text2 = loro2.getText("text");

  // console.log("[1] Insert T to #0");
  text1.insert(0, "T");
  loro1.commit();
  show(text1, loro1, text2, loro2);

  // console.log("[2] Synchronize");
  loro1.import(loro2.exportFrom(loro1.version()));
  loro2.import(loro1.exportFrom(loro2.version()));
  show(text1, loro1, text2, loro2);
  const frontiers1After2 = loro1.frontiers();
  const frontiers2After2 = loro2.frontiers();

  // console.log("[3] Append A to #0");
  text1.insert(1, "A");
  loro1.commit();
  show(text1, loro1, text2, loro2);

  // console.log("[4] Append B to #1");
  text2.insert(1, "B");
  loro2.commit();
  show(text1, loro1, text2, loro2);

  // console.log("[5] Play back to the frontiers after 2");
  loro1.checkout(frontiers1After2);
  loro2.checkout(frontiers2After2);
  show(text1, loro1, text2, loro2);

  // console.log("[6] Check both to the latest");
  loro1.checkoutToLatest();
  loro2.checkoutToLatest();
  show(text1, loro1, text2, loro2);
  const frontiers1Before7 = loro1.frontiers();
  const frontiers2Before7 = loro2.frontiers();

  // console.log("[7] Append B to #1");
  text2.insert(2, "B");
  loro2.commit();
  show(text1, loro1, text2, loro2);

  // console.log("[8] Play back to the frontiers before 7");
  // console.log("----------------------------------------------------------");
  loro1.checkout(frontiers1Before7);
  // console.log("----------------------------------------------------------");
  loro2.checkout(frontiers2Before7);
  show(text1, loro1, text2, loro2);
});

function show(text1: LoroText, loro1: Loro, text2: LoroText, loro2: Loro) {
  // console.log(`    #0 has content: ${JSON.stringify(text1.toString())}`);
  // console.log(`    #0 has frontiers: ${showFrontiers(loro1.frontiers())}`);
  // console.log(`    #1 has content: ${JSON.stringify(text2.toString())}`);
  // console.log(`    #1 has frontiers: ${showFrontiers(loro2.frontiers())}`);
}

function showFrontiers(frontiers: OpId[]) {
  return frontiers.map((x) => `${x.peer}@${x.counter}`).join(";");
}
