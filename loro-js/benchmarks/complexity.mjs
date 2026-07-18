/* eslint-disable no-console */

import { performance } from "node:perf_hooks";

import { decodeImportBlobMeta, LoroDoc, LoroText } from "../dist/index.js";

const positionalArguments = process.argv.slice(2).filter((argument) => argument !== "--");
const sizes = (positionalArguments[0] ?? "1000,2000,4000,8000").split(",").map(Number);

function measure(name, size, callback) {
  globalThis.gc?.();
  const start = performance.now();
  const result = callback();
  const milliseconds = performance.now() - start;
  console.log(JSON.stringify({ name, size, milliseconds, result }));
}

for (const size of sizes) {
  const iterText = new LoroText();
  iterText._sequence.insertAtPhysical(
    0,
    Array.from({ length: size }, (_, index) => ({
      id: { peer: BigInt((index & 1) + 1), counter: index >>> 1 },
      lamport: index,
      value: "x",
      deleted: false,
      originLeft: undefined,
      originRight: undefined,
    })),
  );
  measure("text-iter-first-run", size, () => {
    let chunks = 0;
    iterText.iter(() => {
      chunks += 1;
      return false;
    });
    return chunks;
  });
  measure("text-to-string", size, () => iterText.toString().length);
  measure("text-slice-middle", size, () => {
    const edge = size >>> 2;
    return iterText.slice(edge, size - edge).length;
  });

  const causalDoc = new LoroDoc();
  const causalSequence = causalDoc.getList("causal")._sequence;
  causalSequence.insertAtPhysical(
    0,
    Array.from({ length: size }, (_, counter) => ({
      id: { peer: 99n, counter },
      deleted: false,
      value: counter,
    })),
  );
  measure("sequence-visible-id-runs-1000", size, () => {
    let checksum = 0;
    for (let index = 0; index < 1_000; index += 1) {
      const runs = causalSequence.visibleIdRuns(0, size);
      checksum += runs.length + runs[0].length;
    }
    return checksum;
  });
  measure("sequence-causal-view-tail-cold", size, () => {
    const view = causalSequence.causalView(new Map([[99n, size - 1]]));
    return view.length;
  });
  const excludedCausalVersion = new Map([[99n, 0]]);
  let excludedCausalView;
  measure("sequence-causal-view-cold", size, () => {
    excludedCausalView = causalSequence.causalView(excludedCausalVersion);
    return excludedCausalView.length;
  });
  measure("sequence-causal-view-repeat-1000", size, () => {
    let checksum = 0;
    for (let index = 0; index < 1_000; index += 1) {
      const view = causalSequence.causalView(new Map([[99n, 0]]));
      if (view !== excludedCausalView) throw new Error("causal view cache missed");
      checksum += view.length;
    }
    return checksum;
  });
  const alternatingCausalVersions = [
    new Map([[99n, Math.floor(size / 3)]]),
    new Map([[99n, Math.floor((size * 2) / 3)]]),
  ];
  const alternatingCausalViews = alternatingCausalVersions.map((version) =>
    causalSequence.causalView(version),
  );
  measure("sequence-causal-view-alternate-1000", size, () => {
    let checksum = 0;
    for (let index = 0; index < 1_000; index += 1) {
      const selected = index & 1;
      const view = causalSequence.causalView(alternatingCausalVersions[selected]);
      if (view !== alternatingCausalViews[selected]) {
        throw new Error("alternating causal view cache missed");
      }
      checksum += view.length;
    }
    return checksum;
  });

  measure("list-middle-insert", size, () => {
    const doc = new LoroDoc();
    doc.setPeerId(1);
    const list = doc.getList("list");
    for (let index = 0; index < size; index += 1) {
      list.insert(list.length >>> 1, index);
    }
    return list.length;
  });

  let mapDoc;
  measure("map-reverse-insert", size, () => {
    mapDoc = new LoroDoc();
    mapDoc.setPeerId(1);
    const map = mapDoc.getMap("map");
    for (let index = size - 1; index >= 0; index -= 1) {
      map.set(String(index).padStart(8, "0"), index);
    }
    return map.size;
  });
  measure("map-jsonpath-key-lookup-1000", size, () => {
    let checksum = 0;
    const path = `$.map.${String(size - 1).padStart(8, "0")}`;
    for (let index = 0; index < 1_000; index += 1) {
      checksum += mapDoc.JSONPath(path)[0];
    }
    return checksum;
  });
  mapDoc = undefined;

  let rootsDoc;
  measure("root-map-create", size, () => {
    rootsDoc = new LoroDoc();
    for (let index = 0; index < size; index += 1) {
      rootsDoc.getMap(`root-${index}`);
    }
    return size;
  });
  measure("root-jsonpath-key-lookup-1000", size, () => {
    let found = 0;
    const path = `$.root-${size - 1}`;
    for (let index = 0; index < 1_000; index += 1) {
      if (rootsDoc.JSONPath(path).length === 1) found += 1;
    }
    return found;
  });
  measure("container-unsubscribed-unrelated-commit", size, () => {
    rootsDoc.getMap(`root-${size - 1}`).set("baseline", true);
    rootsDoc.commit();
    return 1;
  });
  const rootSubscriptions = Array.from({ length: size }, (_, index) =>
    rootsDoc.getMap(`root-${index}`).subscribe(() => {}),
  );
  rootsDoc.getMap(`root-${size - 1}`).set("warmup", true);
  rootsDoc.commit();
  measure("container-subscriber-unrelated-commit", size, () => {
    rootsDoc.getMap(`root-${size - 1}`).set("changed", true);
    rootsDoc.commit();
    return 1;
  });
  for (const unsubscribe of rootSubscriptions) unsubscribe();
  rootsDoc = undefined;

  let frontierDoc;
  measure("version-import-independent-peers", size, () => {
    frontierDoc = new LoroDoc();
    frontierDoc.importJsonUpdates({
      schema_version: 1,
      start_version: {},
      peers: null,
      changes: Array.from({ length: size }, (_, index) => {
        const peer = String(index + 1);
        return {
          id: `0@${peer}`,
          timestamp: 0,
          deps: [],
          lamport: 0,
          msg: null,
          ops: [
            {
              container: "cid:root-map:Map",
              counter: 0,
              content: { type: "insert", key: peer, value: index },
            },
          ],
        };
      }),
    });
    return frontierDoc.changeCount();
  });
  measure("version-frontiers-independent-peers", size, () => {
    return frontierDoc.vvToFrontiers(frontierDoc.oplogVersion()).length;
  });
  const frontierSnapshot = frontierDoc.export({ mode: "snapshot" });
  measure("snapshot-metadata-independent-peers", size, () => {
    return decodeImportBlobMeta(frontierSnapshot).partialEndVersionVector.length();
  });
  frontierDoc = undefined;

  let treeDoc;
  measure("tree-root-append", size, () => {
    treeDoc = new LoroDoc();
    treeDoc.setPeerId(1);
    const tree = treeDoc.getTree("tree");
    for (let index = 0; index < size; index += 1) tree.createNode();
    return tree.roots().length;
  });
  measure("tree-root-path-lookup-1000", size, () => {
    let found = 0;
    const path = `tree/${size - 1}`;
    for (let index = 0; index < 1_000; index += 1) {
      if (treeDoc.getByPath(path) !== undefined) found += 1;
    }
    return found;
  });
  measure("tree-root-jsonpath-lookup-1000", size, () => {
    let found = 0;
    const path = `$.tree[${size - 1}]`;
    for (let index = 0; index < 1_000; index += 1) {
      if (treeDoc.JSONPath(path).length === 1) found += 1;
    }
    return found;
  });
  measure("tree-root-to-json", size, () => treeDoc.getTree("tree").toJSON().length);
  treeDoc = undefined;

  const editDoc = new LoroDoc();
  editDoc.setPeerId(1);
  const editText = editDoc.getText("text");
  editText.insert(0, "x".repeat(size));
  editDoc.commit();
  const editFrontiers = editDoc.frontiers();
  measure("text-single-edit", size, () => {
    editText.insert(editText.length >>> 1, "y");
    editDoc.commit();
    return editText.length;
  });
  const unsubscribe = editDoc.subscribe(() => {});
  measure("text-single-edit-subscribed", size, () => {
    editText.insert(editText.length >>> 1, "z");
    editDoc.commit();
    return editText.length;
  });
  unsubscribe();
  measure("text-tail-retreat", size, () => {
    editDoc.checkout(editFrontiers);
    return editText.length;
  });
  measure("text-tail-restore", size, () => {
    editDoc.checkoutToLatest();
    return editText.length;
  });

  const versionedInsertDoc = new LoroDoc();
  versionedInsertDoc.setPeerId(35);
  const versionedInsertText = versionedInsertDoc.getText("text");
  const beforeFullInsert = versionedInsertDoc.frontiers();
  versionedInsertText.insert(0, "x".repeat(size));
  versionedInsertDoc.commit();
  measure("text-full-range-insert-retreat", size, () => {
    versionedInsertDoc.checkout(beforeFullInsert);
    return versionedInsertText.length;
  });
  measure("text-full-range-insert-restore", size, () => {
    versionedInsertDoc.checkoutToLatest();
    return versionedInsertText.length;
  });
  const unsubscribeVersionedInsert = versionedInsertDoc.subscribe(() => {});
  measure("text-full-range-insert-retreat-subscribed", size, () => {
    versionedInsertDoc.checkout(beforeFullInsert);
    return versionedInsertText.length;
  });
  measure("text-full-range-insert-restore-subscribed", size, () => {
    versionedInsertDoc.checkoutToLatest();
    return versionedInsertText.length;
  });
  unsubscribeVersionedInsert();

  const deleteDoc = new LoroDoc();
  deleteDoc.setPeerId(6);
  const deleteText = deleteDoc.getText("text");
  deleteText.insert(0, "x".repeat(size));
  deleteDoc.commit();
  measure("text-id-span-delete", size, () => {
    deleteText.delete(0, size);
    deleteDoc.commit();
    return deleteText.length;
  });
  const subscribedDeleteDoc = new LoroDoc();
  subscribedDeleteDoc.setPeerId(26);
  const subscribedDeleteText = subscribedDeleteDoc.getText("text");
  subscribedDeleteText.insert(0, "x".repeat(size));
  subscribedDeleteDoc.commit();
  const unsubscribeDelete = subscribedDeleteDoc.subscribe(() => {});
  measure("text-id-span-delete-subscribed", size, () => {
    subscribedDeleteText.delete(0, size);
    subscribedDeleteDoc.commit();
    return subscribedDeleteText.length;
  });
  unsubscribeDelete();

  const versionedDeleteDoc = new LoroDoc();
  versionedDeleteDoc.setPeerId(36);
  const versionedDeleteText = versionedDeleteDoc.getText("text");
  versionedDeleteText.insert(0, "x".repeat(size));
  versionedDeleteDoc.commit();
  const beforeFullDelete = versionedDeleteDoc.frontiers();
  versionedDeleteText.delete(0, size);
  versionedDeleteDoc.commit();
  measure("text-full-range-delete-retreat", size, () => {
    versionedDeleteDoc.checkout(beforeFullDelete);
    return versionedDeleteText.length;
  });
  measure("text-full-range-delete-restore", size, () => {
    versionedDeleteDoc.checkoutToLatest();
    return versionedDeleteText.length;
  });
  const unsubscribeVersionedDelete = versionedDeleteDoc.subscribe(() => {});
  measure("text-full-range-delete-retreat-subscribed", size, () => {
    versionedDeleteDoc.checkout(beforeFullDelete);
    return versionedDeleteText.length;
  });
  measure("text-full-range-delete-restore-subscribed", size, () => {
    versionedDeleteDoc.checkoutToLatest();
    return versionedDeleteText.length;
  });
  unsubscribeVersionedDelete();

  const deleteMarkDoc = new LoroDoc();
  deleteMarkDoc.setPeerId(37);
  const deleteMarkText = deleteMarkDoc.getText("text");
  deleteMarkText.insert(0, "x".repeat(size));
  deleteMarkDoc.commit();
  const beforeDeleteMark = deleteMarkDoc.frontiers();
  deleteMarkText.mark({ start: 0, end: size }, "bold", true);
  deleteMarkText.delete(0, size);
  deleteMarkDoc.commit();
  deleteMarkDoc.checkout(beforeDeleteMark);
  const unsubscribeDeleteMark = deleteMarkDoc.subscribe(() => {});
  measure("text-full-range-delete-mark-forward-subscribed", size, () => {
    deleteMarkDoc.checkoutToLatest();
    return deleteMarkText.length;
  });
  unsubscribeDeleteMark();

  const scalarDeleteDoc = new LoroDoc();
  scalarDeleteDoc.setPeerId(7);
  const scalarDeleteText = scalarDeleteDoc.getText("text");
  scalarDeleteText.insert(0, "x".repeat(size));
  scalarDeleteDoc.commit();
  const scalarDeleteElements = scalarDeleteText._sequence.all();
  measure("text-id-span-delete-scalar-reference", size, () => {
    for (let index = 0; index < scalarDeleteElements.length; index += 1) {
      const element = scalarDeleteElements[index];
      scalarDeleteText._sequence.setDeleted(element, true);
      scalarDeleteText._sequence.addDeletion(element, { peer: 8n, counter: index });
    }
    return scalarDeleteText.length;
  });

  const cursorDoc = new LoroDoc();
  cursorDoc.setPeerId(16);
  const cursorText = cursorDoc.getText("text");
  cursorText.insert(0, "x".repeat(size));
  const deletedCursor = cursorText.getCursor(0);
  if (deletedCursor === undefined) throw new Error("cursor benchmark requires size > 0");
  cursorText.delete(0, Math.max(0, size - 1));
  cursorDoc.commit();
  measure("cursor-deleted-gap-lookup-1000", size, () => {
    let checksum = 0;
    for (let index = 0; index < 1_000; index += 1) {
      checksum += cursorDoc.getCursorPos(deletedCursor)?.offset ?? 0;
    }
    return checksum;
  });

  const fullMarkDoc = new LoroDoc();
  fullMarkDoc.setPeerId(24);
  const fullMarkText = fullMarkDoc.getText("text");
  fullMarkText.insert(0, "x".repeat(size));
  fullMarkDoc.commit();
  const beforeFullMark = fullMarkDoc.frontiers();
  measure("text-full-range-mark", size, () => {
    fullMarkText.mark({ start: 0, end: size }, "bold", true);
    fullMarkDoc.commit();
    return fullMarkText.length;
  });
  const subscribedMarkDoc = new LoroDoc();
  subscribedMarkDoc.setPeerId(27);
  const subscribedMarkText = subscribedMarkDoc.getText("text");
  subscribedMarkText.insert(0, "x".repeat(size));
  subscribedMarkDoc.commit();
  const unsubscribeMark = subscribedMarkDoc.subscribe(() => {});
  measure("text-full-range-mark-subscribed", size, () => {
    subscribedMarkText.mark({ start: 0, end: size }, "bold", true);
    subscribedMarkDoc.commit();
    return subscribedMarkText.length;
  });
  unsubscribeMark();
  measure("text-full-range-mark-retreat", size, () => {
    fullMarkDoc.checkout(beforeFullMark);
    return fullMarkText.length;
  });
  measure("text-full-range-mark-restore", size, () => {
    fullMarkDoc.checkoutToLatest();
    return fullMarkText.length;
  });
  fullMarkDoc.checkout(beforeFullMark);
  const unsubscribeFullMark = fullMarkDoc.subscribe(() => {});
  measure("text-full-range-mark-restore-subscribed", size, () => {
    fullMarkDoc.checkoutToLatest();
    return fullMarkText.length;
  });
  unsubscribeFullMark();

  const markDoc = new LoroDoc();
  markDoc.setPeerId(4);
  const markText = markDoc.getText("text");
  markText.insert(0, "x".repeat(size));
  markDoc.commit();
  const beforeMark = markDoc.frontiers();
  markText.mark({ start: size >>> 1, end: (size >>> 1) + 1 }, "bold", true);
  markDoc.commit();
  measure("text-mark-tail-retreat", size, () => {
    markDoc.checkout(beforeMark);
    return markText.length;
  });
  measure("text-mark-tail-restore", size, () => {
    markDoc.checkoutToLatest();
    return markText.length;
  });

  const markHistoryDoc = new LoroDoc();
  markHistoryDoc.setPeerId(14);
  const markHistoryText = markHistoryDoc.getText("text");
  markHistoryText.insert(0, "x");
  markHistoryDoc.commit();
  let beforeLastMark;
  for (let index = 0; index < size; index += 1) {
    if (index === size - 1) beforeLastMark = markHistoryDoc.frontiers();
    markHistoryText.mark({ start: 0, end: 1 }, "bold", (index & 1) === 0);
    markHistoryDoc.commit();
  }
  measure("text-repeated-mark-tail-retreat", size, () => {
    markHistoryDoc.checkout(beforeLastMark);
    return markHistoryText.length;
  });
  measure("text-repeated-mark-tail-restore", size, () => {
    markHistoryDoc.checkoutToLatest();
    return markHistoryText.length;
  });

  const movableDoc = new LoroDoc();
  movableDoc.setPeerId(5);
  const movable = movableDoc.getMovableList("list");
  for (let index = 0; index < size; index += 1) movable.push(index);
  movableDoc.commit();
  const beforeMove = movableDoc.frontiers();
  movable.move(0, movable.length - 1);
  movableDoc.commit();
  measure("movable-tail-retreat", size, () => {
    movableDoc.checkout(beforeMove);
    return movable.length;
  });
  measure("movable-tail-restore", size, () => {
    movableDoc.checkoutToLatest();
    return movable.length;
  });

  const movableValueDoc = new LoroDoc();
  movableValueDoc.setPeerId(15);
  const movableValue = movableValueDoc.getMovableList("list");
  movableValue.push(-1);
  movableValueDoc.commit();
  let beforeLastSet;
  for (let index = 0; index < size; index += 1) {
    if (index === size - 1) beforeLastSet = movableValueDoc.frontiers();
    movableValue.set(0, index);
    movableValueDoc.commit();
  }
  measure("movable-repeated-set-tail-retreat", size, () => {
    movableValueDoc.checkout(beforeLastSet);
    return movableValue.get(0);
  });
  measure("movable-repeated-set-tail-restore", size, () => {
    movableValueDoc.checkoutToLatest();
    return movableValue.get(0);
  });

  const importSource = new LoroDoc();
  importSource.setPeerId(2);
  importSource.getText("text").insert(0, "x".repeat(size));
  importSource.commit();
  const importTarget = new LoroDoc();
  importTarget.import(importSource.export({ mode: "update" }));
  const importVersion = importTarget.oplogVersion();
  importSource.getText("text").insert(size >>> 1, "y");
  importSource.commit();
  const tailTextUpdate = importSource.export({
    mode: "update",
    from: importVersion,
  });
  const unsubscribeImport = importTarget.subscribe(() => {});
  measure("text-tail-import-subscribed", size, () => {
    importTarget.import(tailTextUpdate);
    return importTarget.getText("text").length;
  });
  unsubscribeImport();

  const checkoutTextDoc = new LoroDoc();
  checkoutTextDoc.setPeerId(3);
  checkoutTextDoc.getText("text").insert(0, "x".repeat(size));
  checkoutTextDoc.commit();
  const checkoutTextFrontiers = checkoutTextDoc.frontiers();
  checkoutTextDoc.getText("text").insert(size >>> 1, "y");
  checkoutTextDoc.commit();
  checkoutTextDoc.checkout(checkoutTextFrontiers);
  measure("text-tail-checkout", size, () => {
    checkoutTextDoc.checkoutToLatest();
    return checkoutTextDoc.getText("text").length;
  });
  checkoutTextDoc.checkout(checkoutTextFrontiers);
  const unsubscribeCheckout = checkoutTextDoc.subscribe(() => {});
  measure("text-tail-checkout-subscribed", size, () => {
    checkoutTextDoc.checkoutToLatest();
    return checkoutTextDoc.getText("text").length;
  });
  unsubscribeCheckout();

  measure("text-subscribed-batch", size, () => {
    const doc = new LoroDoc();
    doc.setPeerId(1);
    doc.subscribe(() => {});
    const text = doc.getText("text");
    for (let index = 0; index < size; index += 1) {
      text.insert(text.length >>> 1, "x");
    }
    doc.commit();
    return text.length;
  });

  let historyDoc;
  let tailFrontiers;
  let tailVersion;
  measure("history-commit", size, () => {
    historyDoc = new LoroDoc();
    historyDoc.setPeerId(1);
    const counter = historyDoc.getCounter("counter");
    for (let index = 0; index < size; index += 1) {
      if (index === size - 1) {
        tailFrontiers = historyDoc.frontiers();
        tailVersion = historyDoc.oplogVersion();
      }
      counter.increment(1);
      historyDoc.commit({ message: String(index) });
    }
    return historyDoc.changeCount();
  });
  let tailUpdate;
  measure("history-tail-export", size, () => {
    tailUpdate = historyDoc.export({ mode: "update", from: tailVersion });
    return tailUpdate.byteLength;
  });
  measure(
    "history-tail-span-export",
    size,
    () =>
      historyDoc.export({
        mode: "updates-in-range",
        spans: [{ id: { peer: "1", counter: size - 1 }, len: 1 }],
      }).byteLength,
  );
  const tailTarget = historyDoc.forkAt(tailFrontiers);
  measure("history-tail-import", size, () => {
    tailTarget.import(tailUpdate);
    return tailTarget.oplogVersion().get("1");
  });
  const checkoutTarget = historyDoc.fork();
  measure("history-tail-retreat", size, () => {
    checkoutTarget.checkout(tailFrontiers);
    return checkoutTarget.version().get("1");
  });
  measure("history-tail-checkout", size, () => {
    checkoutTarget.checkoutToLatest();
    return checkoutTarget.version().get("1");
  });
  measure("history-tail-diff", size, () => {
    return historyDoc.diff(tailFrontiers, historyDoc.frontiers(), false).length;
  });
  measure("history-get-all-changes", size, () => {
    let changes = 0;
    for (const peerChanges of historyDoc.getAllChanges().values()) {
      changes += peerChanges.length;
    }
    return changes;
  });
  const singleOperationUpdates = Array.from({ length: size }, (_, counter) =>
    historyDoc.export({
      mode: "updates-in-range",
      spans: [{ id: { peer: "1", counter }, len: 1 }],
    }),
  );
  measure("history-update-batch-import", size, () => {
    const target = new LoroDoc();
    target.importBatch(singleOperationUpdates.reverse());
    return target.opCount();
  });
  measure("history-indexed-queries", size, () => {
    let checksum = 0;
    for (let index = 0; index < size; index += 1) {
      checksum += historyDoc.version().length();
      checksum += historyDoc.frontiers().length;
      checksum += historyDoc.getChangeAt({ peer: "1", counter: index }).length;
    }
    return checksum;
  });

  const movableBase = historyDoc.fork();
  const movableBaseList = movableBase.getMovableList("movable");
  for (const value of ["a", "b", "c", "d"]) movableBaseList.push(value);
  movableBase.commit();
  const movableLeft = movableBase.fork();
  movableLeft.setPeerId(2);
  movableLeft.getMovableList("movable").move(0, 3);
  movableLeft.commit();
  const movableLeftFrontiers = movableLeft.frontiers();
  const movableRight = movableBase.fork();
  movableRight.setPeerId(3);
  movableRight.getMovableList("movable").move(3, 0);
  movableRight.commit();
  const movableRightFrontiers = movableRight.frontiers();
  movableLeft.import(movableRight.export({ mode: "update" }));
  movableLeft.checkout(movableLeftFrontiers);
  measure("history-concurrent-movable-switch", size, () => {
    movableLeft.checkout(movableRightFrontiers);
    return movableLeft.getMovableList("movable").length;
  });
  movableLeft.checkout(movableLeftFrontiers);
  const unsubscribeMovableSwitch = movableLeft.subscribe(() => {});
  measure("history-concurrent-movable-switch-subscribed", size, () => {
    movableLeft.checkout(movableRightFrontiers);
    return movableLeft.getMovableList("movable").length;
  });
  unsubscribeMovableSwitch();

  const mixedLeft = movableBase.fork();
  mixedLeft.setPeerId(4);
  const mixedLeftList = mixedLeft.getMovableList("movable");
  mixedLeftList.move(0, 3);
  mixedLeftList.insert(1, "left");
  mixedLeft.commit();
  const mixedLeftFrontiers = mixedLeft.frontiers();
  const mixedRight = movableBase.fork();
  mixedRight.setPeerId(5);
  const mixedRightList = mixedRight.getMovableList("movable");
  mixedRightList.move(3, 0);
  mixedRightList.delete(2, 1);
  mixedRight.commit();
  const mixedRightFrontiers = mixedRight.frontiers();
  mixedLeft.import(mixedRight.export({ mode: "update" }));
  mixedLeft.checkout(mixedLeftFrontiers);
  measure("history-concurrent-movable-mixed-switch", size, () => {
    mixedLeft.checkout(mixedRightFrontiers);
    return mixedLeft.getMovableList("movable").length;
  });
  mixedLeft.checkout(mixedLeftFrontiers);
  const unsubscribeMixedSwitch = mixedLeft.subscribe(() => {});
  measure("history-concurrent-movable-mixed-switch-subscribed", size, () => {
    mixedLeft.checkout(mixedRightFrontiers);
    return mixedLeft.getMovableList("movable").length;
  });
  unsubscribeMixedSwitch();

  const lookupRoot = historyDoc.getMap("lookup");
  const preserved = lookupRoot.ensureMergeableText("preserved");
  preserved.insert(0, "x");
  historyDoc.commit();
  lookupRoot.delete("preserved");
  historyDoc.commit();
  measure("history-container-id-lookup-1000", size, () => {
    let found = 0;
    for (let index = 0; index < 1_000; index += 1) {
      if (historyDoc.hasContainer(preserved.id)) found += 1;
    }
    return found;
  });
}
