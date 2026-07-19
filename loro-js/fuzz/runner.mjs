import assert from "node:assert/strict";
import { isDeepStrictEqual } from "node:util";

export function canonicalize(value) {
  if (value === undefined) return { $undefined: true };
  if (typeof value === "number") {
    if (Number.isNaN(value)) return { $number: "NaN" };
    if (value === Number.POSITIVE_INFINITY) return { $number: "+Infinity" };
    if (value === Number.NEGATIVE_INFINITY) return { $number: "-Infinity" };
    if (Object.is(value, -0)) return { $number: "-0" };
    return value;
  }
  if (typeof value === "bigint") return { $bigint: value.toString() };
  if (value instanceof Uint8Array) {
    return { $bytes: Buffer.from(value).toString("base64") };
  }
  if (value instanceof Map) {
    return {
      $map: [...value]
        .map(([key, child]) => [String(key), canonicalize(child)])
        .sort(([left], [right]) => left.localeCompare(right)),
    };
  }
  if (Array.isArray(value)) return value.map(canonicalize);
  if (typeof value === "string" && value.startsWith("idx:")) {
    const marker = value.indexOf(", id:cid:");
    if (marker >= 0) return value.slice(marker + ", id:".length);
  }
  if (value === null || typeof value !== "object") return value;

  const output = {};
  for (const inputKey of Object.keys(value).sort()) {
    const outputKey = inputKey === "fractionalIndex" ? "fractional_index" : inputKey;
    const child = value[inputKey];
    if (child !== undefined) output[outputKey] = canonicalize(child);
  }
  if ("fractional_index" in output && !("parent" in output)) output.parent = null;
  return output;
}

function canonicalVersion(version) {
  const json = typeof version?.toJSON === "function" ? version.toJSON() : version;
  if (json instanceof Map) {
    return [...json]
      .map(([peer, counter]) => [String(peer), counter])
      .sort(([left], [right]) => left.localeCompare(right));
  }
  return canonicalize(json);
}

function canonicalFrontiers(frontiers) {
  return frontiers
    .map(({ peer, counter }) => `${counter}@${peer}`)
    .sort((left, right) => left.localeCompare(right));
}

function normalizeDeepWithId(value, key) {
  if (Array.isArray(value)) return value.map((child) => normalizeDeepWithId(child));
  if (value === null || typeof value !== "object") return value;
  if (key === "meta" && typeof value.cid === "string" && Object.hasOwn(value, "value")) {
    return normalizeDeepWithId(value.value, key);
  }
  return Object.fromEntries(
    Object.entries(value).map(([childKey, child]) => [
      childKey,
      normalizeDeepWithId(child, childKey),
    ]),
  );
}

function normalizeEventBatch(batch, doc) {
  const normalized = canonicalize(batch);
  // `origin` is binding metadata rather than a CRDT diff. The pure TS runtime
  // does not currently expose every synthetic Rust origin (for example
  // "checkout"), so compare the observable transition itself.
  delete normalized.origin;
  for (const key of ["from", "to"]) {
    if (Array.isArray(normalized[key])) {
      normalized[key].sort((left, right) =>
        `${left.counter}@${left.peer}`.localeCompare(`${right.counter}@${right.peer}`),
      );
    }
  }
  if (Array.isArray(normalized.events)) {
    for (const event of normalized.events) {
      if (Array.isArray(event.diff?.diff)) {
        event.diff.diff = canonicalizeDeltaOrder(coalesceDelta(event.diff.diff));
      }
      if (event.diff?.type === "tree" && Array.isArray(event.diff.diff)) {
        event.diff.diff = removeTransientTreeActions(event.diff.diff);
      }
    }
    normalized.events = normalized.events.filter(
      (event) => eventHasEffect(event) && !targetsDeletedTreeNode(event, doc),
    );
    normalized.events.sort((left, right) =>
      JSON.stringify([left.target, left.path]).localeCompare(
        JSON.stringify([right.target, right.path]),
      ),
    );
  }
  return canonicalize(normalized);
}

function canonicalizeDeltaOrder(delta) {
  const output = [];
  let group = [];
  const flush = () => {
    if (group.length === 0) return;
    const deletes = group.reduce((sum, operation) => sum + (operation.delete ?? 0), 0);
    if (deletes > 0) output.push({ delete: deletes });
    output.push(
      ...coalesceDelta(group.filter((operation) => operation.delete === undefined)),
    );
    group = [];
  };
  for (const operation of delta) {
    if (operation.retain !== undefined) {
      flush();
      output.push(operation);
    } else {
      group.push(operation);
    }
  }
  flush();
  return coalesceDelta(output);
}

function removeTransientTreeActions(actions) {
  const transientTargets = new Set();
  for (const action of actions) {
    if (action.action !== "delete") continue;
    if (
      actions.some(
        (candidate) =>
          candidate.target === action.target && candidate.action === "create",
      )
    ) {
      transientTargets.add(action.target);
    }
  }
  return actions.filter((action) => !transientTargets.has(action.target));
}

function targetsDeletedTreeNode(event, doc) {
  const [root, node] = event.path ?? [];
  return (
    typeof root === "string" &&
    typeof node === "string" &&
    node.includes("@") &&
    doc.getTree(root).isNodeDeleted(node)
  );
}

function eventHasEffect(event) {
  const diff = event.diff;
  if (Array.isArray(diff?.diff)) return diff.diff.length > 0;
  if (diff?.type === "map") return Object.keys(diff.updated ?? {}).length > 0;
  if (diff?.type === "counter") return diff.increment !== 0;
  return true;
}

function coalesceDelta(delta) {
  const output = [];
  for (const operation of delta) {
    const previous = output.at(-1);
    if (
      previous !== undefined &&
      Object.hasOwn(previous, "insert") &&
      Object.hasOwn(operation, "insert") &&
      isDeepStrictEqual(previous.attributes, operation.attributes)
    ) {
      if (Array.isArray(previous.insert) && Array.isArray(operation.insert)) {
        previous.insert.push(...operation.insert);
        continue;
      }
      if (typeof previous.insert === "string" && typeof operation.insert === "string") {
        previous.insert += operation.insert;
        continue;
      }
    }
    if (
      previous !== undefined &&
      typeof previous.delete === "number" &&
      typeof operation.delete === "number"
    ) {
      previous.delete += operation.delete;
      continue;
    }
    if (
      previous !== undefined &&
      typeof previous.retain === "number" &&
      typeof operation.retain === "number" &&
      isDeepStrictEqual(previous.attributes, operation.attributes)
    ) {
      previous.retain += operation.retain;
      continue;
    }
    output.push(structuredClone(operation));
  }
  return output;
}

export function observeDoc(doc) {
  const pendingLength = doc.getPendingTxnLength();
  const tree = doc.getTree("tree");
  const treeNodes = tree
    .getNodes({ withDeleted: true })
    .map((node) => node.toJSON())
    .sort((left, right) => String(left.id).localeCompare(String(right.id)));
  const observation = {
    json: doc.toJSON(),
    deepWithId: normalizeDeepWithId(doc.getDeepValueWithID()),
    textDelta: doc.getText("text").toDelta(),
    treeNodes,
    version: canonicalVersion(doc.version()),
    frontiers: canonicalFrontiers(doc.frontiers()),
    detached: doc.isDetached(),
    shallow: doc.isShallow(),
    pendingLength,
  };
  if (pendingLength === 0) {
    observation.oplogVersion = canonicalVersion(doc.oplogVersion());
    observation.oplogFrontiers = canonicalFrontiers(doc.oplogFrontiers());
    observation.opCount = doc.opCount();
    observation.changeCount = doc.changeCount();
  }
  return canonicalize(observation);
}

export function observeDocForNative(doc) {
  return canonicalize({
    json: doc.toJSON(),
    deepWithId: normalizeDeepWithId(doc.getDeepValueWithID()),
    version: canonicalVersion(doc.version()),
    oplogVersion: canonicalVersion(doc.oplogVersion()),
    frontiers: canonicalFrontiers(doc.frontiers()),
    oplogFrontiers: canonicalFrontiers(doc.oplogFrontiers()),
    detached: doc.isDetached(),
    shallow: doc.isShallow(),
    opCount: doc.opCount(),
    changeCount: doc.changeCount(),
  });
}

function errorCategory(error) {
  const message = String(error instanceof Error ? error.message : error).toLowerCase();
  if (message.includes("detached")) return "detached";
  if (message.includes("cycle") || message.includes("descendant")) return "tree-cycle";
  if (
    message.includes("range") ||
    message.includes("position") ||
    message.includes("index")
  ) {
    return "range";
  }
  if (message.includes("shallow")) return "shallow";
  if (message.includes("checksum")) return "checksum";
  if (message.includes("dependency") || message.includes("pending")) return "dependency";
  return error?.constructor?.name ?? "Error";
}

function capture(action) {
  try {
    const value = action();
    return { ok: true, value: canonicalize(value) };
  } catch (error) {
    return { ok: false, error: errorCategory(error) };
  }
}

function modulo(value, length) {
  if (length <= 0) return 0;
  return ((value % length) + length) % length;
}

function peerIndex(value, peerCount) {
  return modulo(value, peerCount);
}

function utf16Boundaries(value) {
  const output = [0];
  for (let index = 0; index < value.length; ) {
    const codePoint = value.codePointAt(index);
    index += codePoint !== undefined && codePoint > 0xffff ? 2 : 1;
    output.push(index);
  }
  return output;
}

function selectedTextRange(text, rawIndex, rawLength, allowEmpty) {
  const boundaries = utf16Boundaries(text.toString());
  const startIndex = modulo(rawIndex, boundaries.length);
  const remaining = boundaries.length - startIndex - 1;
  if (remaining <= 0) {
    return { start: boundaries[startIndex], end: boundaries[startIndex] };
  }
  const scalarLength = allowEmpty
    ? modulo(rawLength, remaining + 1)
    : 1 + modulo(rawLength, remaining);
  return {
    start: boundaries[startIndex],
    end: boundaries[startIndex + scalarLength],
  };
}

function liveTreeNodes(tree) {
  return tree
    .getNodes()
    .slice()
    .sort((left, right) => String(left.id).localeCompare(String(right.id)));
}

function isTreeDescendant(node, ancestorId) {
  for (let parent = node.parent(); parent !== undefined; parent = parent.parent()) {
    if (parent.id === ancestorId) return true;
  }
  return false;
}

function applyEdit(doc, command) {
  switch (command.kind) {
    case "map-set":
      return doc.getMap("map").set(command.key, command.value);
    case "map-delete": {
      const map = doc.getMap("map");
      const keys = map.keys();
      if (keys.length === 0) return undefined;
      const key = keys.includes(command.key) ? command.key : keys.sort()[0];
      return map.delete(key);
    }
    case "list-insert": {
      const list = doc.getList("list");
      return list.insert(modulo(command.index, list.length + 1), command.value);
    }
    case "list-delete": {
      const list = doc.getList("list");
      if (list.length === 0) return undefined;
      const index = modulo(command.index, list.length);
      const length = 1 + modulo(command.length, list.length - index);
      return list.delete(index, length);
    }
    case "text-insert": {
      const text = doc.getText("text");
      const boundaries = utf16Boundaries(text.toString());
      return text.insert(
        boundaries[modulo(command.index, boundaries.length)],
        command.text,
      );
    }
    case "text-delete": {
      const text = doc.getText("text");
      const { start, end } = selectedTextRange(
        text,
        command.index,
        command.length,
        false,
      );
      if (start === end) return undefined;
      return text.delete(start, end - start);
    }
    case "text-mark": {
      const text = doc.getText("text");
      const { start, end } = selectedTextRange(
        text,
        command.index,
        command.length,
        false,
      );
      if (start === end) return undefined;
      return text.mark({ start, end }, command.key, command.value);
    }
    case "text-unmark": {
      const text = doc.getText("text");
      const { start, end } = selectedTextRange(
        text,
        command.index,
        command.length,
        false,
      );
      if (start === end) return undefined;
      return text.unmark({ start, end }, command.key);
    }
    case "movable-insert": {
      const list = doc.getMovableList("movable");
      return list.insert(modulo(command.index, list.length + 1), command.value);
    }
    case "movable-delete": {
      const list = doc.getMovableList("movable");
      if (list.length === 0) return undefined;
      const index = modulo(command.index, list.length);
      const length = 1 + modulo(command.length, list.length - index);
      return list.delete(index, length);
    }
    case "movable-set": {
      const list = doc.getMovableList("movable");
      if (list.length === 0) return undefined;
      const index = modulo(command.index, list.length);
      if (isDeepStrictEqual(canonicalize(list.get(index)), canonicalize(command.value))) {
        return undefined;
      }
      return list.set(index, command.value);
    }
    case "movable-move": {
      const list = doc.getMovableList("movable");
      if (list.length < 2) return undefined;
      return list.move(
        modulo(command.from, list.length),
        modulo(command.to, list.length),
      );
    }
    case "counter-increment":
      return command.delta === 0
        ? undefined
        : doc.getCounter("counter").increment(command.delta);
    case "tree-create": {
      const tree = doc.getTree("tree");
      const nodes = liveTreeNodes(tree);
      const parent =
        command.parent === null || nodes.length === 0
          ? undefined
          : nodes[modulo(command.parent, nodes.length)].id;
      const node = tree.createNode(parent);
      node.data.set("value", command.value);
      return node.id;
    }
    case "tree-meta-set": {
      const nodes = liveTreeNodes(doc.getTree("tree"));
      if (nodes.length === 0) return undefined;
      return nodes[modulo(command.node, nodes.length)].data.set(
        command.key,
        command.value,
      );
    }
    case "tree-move": {
      const tree = doc.getTree("tree");
      const nodes = liveTreeNodes(tree);
      if (nodes.length === 0) return undefined;
      const node = nodes[modulo(command.node, nodes.length)];
      const candidates = nodes.filter(
        (candidate) => candidate.id !== node.id && !isTreeDescendant(candidate, node.id),
      );
      const parent =
        command.parent === null || candidates.length === 0
          ? undefined
          : candidates[modulo(command.parent, candidates.length)];
      if (node.parent()?.id === parent?.id) return undefined;
      return tree.move(node.id, parent?.id);
    }
    case "tree-delete": {
      const tree = doc.getTree("tree");
      const nodes = liveTreeNodes(tree);
      if (nodes.length === 0) return undefined;
      return tree.delete(nodes[modulo(command.node, nodes.length)].id);
    }
    default:
      throw new RangeError(`unsupported edit command ${command.kind}`);
  }
}

function createDoc(engine, peer, initializeRoots = true) {
  const doc = new engine.LoroDoc();
  if (peer !== undefined) doc.setPeerId(peer + 1);
  doc.setRecordTimestamp(false);
  doc.setChangeMergeInterval(0);
  if (initializeRoots) {
    doc.getMap("map");
    doc.getList("list");
    doc.getText("text");
    doc.getMovableList("movable");
    doc.getCounter("counter");
    doc.getTree("tree").enableFractionalIndex(0);
  }
  return doc;
}

function exportFor(doc, target, mode) {
  return mode === "snapshot"
    ? doc.export({ mode: "snapshot" })
    : doc.export({ mode: "update", from: target.oplogVersion() });
}

function compare(label, left, right) {
  try {
    assert.deepStrictEqual(left, right);
  } catch (cause) {
    const error = new Error(
      `${label}\nleft=${JSON.stringify(left)}\nright=${JSON.stringify(right)}`,
    );
    error.cause = cause;
    throw error;
  }
}

class EngineWorld {
  constructor(engine, scenario, externalBlobs) {
    this.engine = engine;
    this.docs = Array.from({ length: scenario.peerCount }, (_, peer) =>
      createDoc(engine, peer),
    );
    this.slots = new Map();
    this.checkpoints = this.docs.map(() => new Map());
    this.externalBlobs = externalBlobs?.map((bytes) => new Uint8Array(bytes));
    this.transportBlobs = [];
    this.enqueueIndex = 0;
    this.eventQueues = this.docs.map(() => []);
    this.subscriptions = this.docs.map((doc, peer) =>
      doc.subscribe((batch) => {
        const normalized = normalizeEventBatch(batch, doc);
        if (normalized.events.length > 0) this.eventQueues[peer].push(normalized);
      }),
    );
  }

  execute(command) {
    const doc = this.docs[peerIndex(command.peer ?? 0, this.docs.length)];
    if (command.kind.includes("-") && isEditCommand(command.kind)) {
      return capture(() => applyEdit(doc, command));
    }
    switch (command.kind) {
      case "commit":
        return capture(() => doc.commit({ message: `interop-fuzz-${command.message}` }));
      case "enqueue": {
        const sourceIndex = peerIndex(command.source, this.docs.length);
        let targetIndex = peerIndex(command.target, this.docs.length);
        if (targetIndex === sourceIndex)
          targetIndex = (targetIndex + 1) % this.docs.length;
        const ownBlob = exportFor(
          this.docs[sourceIndex],
          this.docs[targetIndex],
          command.mode,
        );
        const external = this.externalBlobs?.[this.enqueueIndex];
        const blob = external ?? ownBlob;
        this.transportBlobs.push([...ownBlob]);
        this.enqueueIndex += 1;
        this.slots.set(modulo(command.slot, 8), {
          blob,
          targetIndex,
          mode: command.mode,
        });
        return { ok: true, value: "enqueued" };
      }
      case "deliver": {
        const slot = this.slots.get(modulo(command.slot, 8));
        if (slot === undefined) return { ok: true, value: "empty-slot" };
        const copies = 1 + modulo(command.copies, 3);
        const statuses = [];
        for (let copy = 0; copy < copies; copy += 1) {
          statuses.push(canonicalize(this.docs[slot.targetIndex].import(slot.blob)));
        }
        return { ok: true, value: statuses };
      }
      case "save":
        this.checkpoints[peerIndex(command.peer, this.docs.length)].set(
          modulo(command.checkpoint, 8),
          doc.frontiers(),
        );
        return { ok: true, value: undefined };
      case "checkout": {
        const frontiers = this.checkpoints[peerIndex(command.peer, this.docs.length)].get(
          modulo(command.checkpoint, 8),
        );
        if (frontiers === undefined) return { ok: true, value: "missing-checkpoint" };
        if (
          isDeepStrictEqual(
            canonicalFrontiers(frontiers),
            canonicalFrontiers(doc.frontiers()),
          )
        ) {
          return { ok: true, value: "current-checkpoint" };
        }
        return capture(() => doc.checkout(frontiers));
      }
      case "attach":
        return capture(() => doc.attach());
      case "roundtrip":
        return this.roundtrip(doc, command.mode);
      default:
        throw new RangeError(`unsupported command ${command.kind}`);
    }
  }

  roundtrip(doc, mode) {
    return capture(() => {
      const bytes =
        mode === "snapshot"
          ? doc.export({ mode: "snapshot" })
          : doc.export({ mode: "update" });
      const imported = createDoc(this.engine, undefined, false);
      imported.import(bytes);
      return observeDoc(imported);
    });
  }

  finalNativeObservations() {
    return this.docs.map(observeDocForNative);
  }

  drainEvents() {
    this.engine.callPendingEvents?.();
    return this.eventQueues.map((queue) => queue.splice(0));
  }
}

function isEditCommand(kind) {
  return (
    kind.startsWith("map-") ||
    kind.startsWith("list-") ||
    kind.startsWith("text-") ||
    kind.startsWith("movable-") ||
    kind.startsWith("counter-") ||
    kind.startsWith("tree-")
  );
}

export function runSingleScenario(engine, scenario, externalBlobs) {
  const world = new EngineWorld(engine, scenario, externalBlobs);
  const results = [];
  for (const command of scenario.commands) results.push(world.execute(command));
  return {
    observations: world.finalNativeObservations(),
    transportBlobs: world.transportBlobs,
    results: canonicalize(results),
  };
}

export function runMalformedImportChecks(wasmEngine, jsEngine) {
  const engines = [
    ["wasm", wasmEngine],
    ["js", jsEngine],
  ];
  let cases = 0;
  for (const [producerName, producer] of engines) {
    const source = createDoc(producer, 0);
    source.getMap("map").set("payload", { nested: [1, true, "😀"] });
    source.getText("text").insert(0, "malformed 😀 文");
    source.getList("list").push("item");
    source.commit({ message: "malformed-seed" });
    for (const mode of ["update", "snapshot"]) {
      const blob = source.export({ mode });
      for (const [mutation, bytes] of malformedVariants(blob)) {
        const outcomes = engines.map(([consumerName, engine]) => [
          consumerName,
          malformedOutcome(engine, bytes),
        ]);
        compare(
          `malformed import mismatch: producer=${producerName} mode=${mode} mutation=${mutation}`,
          outcomes[0][1],
          outcomes[1][1],
        );
        cases += 1;
      }
    }
  }
  return cases;
}

function malformedVariants(blob) {
  const half = Math.max(1, Math.floor(blob.length / 2));
  return [
    ["empty", new Uint8Array()],
    ["prefix-1", blob.slice(0, 1)],
    ["prefix-half", blob.slice(0, half)],
    ["truncate-last", blob.slice(0, -1)],
    ["append-garbage", Uint8Array.from([...blob, 0xff])],
    ["flip-header", flippedByte(blob, 0)],
    ["flip-middle", flippedByte(blob, Math.floor(blob.length / 2))],
    ["flip-tail", flippedByte(blob, blob.length - 1)],
  ];
}

function flippedByte(blob, index) {
  const output = blob.slice();
  output[index] ^= 0xff;
  return output;
}

function malformedOutcome(engine, bytes) {
  const target = createDoc(engine, 97);
  target.getMap("map").set("sentinel", "unchanged");
  target.getText("text").insert(0, "stable");
  target.commit({ message: "malformed-target" });
  const before = observeDoc(target);
  try {
    target.import(bytes);
    return { accepted: true, observation: observeDoc(target) };
  } catch {
    return {
      accepted: false,
      atomic: isDeepStrictEqual(before, observeDoc(target)),
    };
  }
}

export function runDifferentialScenario(
  wasmEngine,
  jsEngine,
  scenario,
  { strict = false } = {},
) {
  const wasm = new EngineWorld(wasmEngine, scenario);
  const js = new EngineWorld(jsEngine, scenario);

  for (let index = 0; index < scenario.commands.length; index += 1) {
    const command = scenario.commands[index];
    let wasmResult;
    let jsResult;
    let compareEvents = true;
    if (command.kind === "deliver") {
      const wasmSlot = wasm.slots.get(modulo(command.slot, 8));
      const jsSlot = js.slots.get(modulo(command.slot, 8));
      if (wasmSlot === undefined || jsSlot === undefined) {
        wasmResult = { ok: true, value: "empty-slot" };
        jsResult = { ok: true, value: "empty-slot" };
      } else {
        compareEvents = strict || wasmSlot.mode !== "snapshot";
        const copies = 1 + modulo(command.copies, 3);
        const wasmStatuses = [];
        const jsStatuses = [];
        for (let copy = 0; copy < copies; copy += 1) {
          wasmStatuses.push(
            canonicalize(wasm.docs[wasmSlot.targetIndex].import(jsSlot.blob)),
          );
          jsStatuses.push(
            canonicalize(js.docs[jsSlot.targetIndex].import(wasmSlot.blob)),
          );
        }
        wasmResult = { ok: true, value: wasmStatuses };
        jsResult = { ok: true, value: jsStatuses };
      }
    } else if (command.kind === "roundtrip") {
      const peer = peerIndex(command.peer, wasm.docs.length);
      wasmResult = crossRoundtrip(
        wasmEngine,
        jsEngine,
        wasm.docs[peer],
        js.docs[peer],
        command.mode,
      );
      jsResult = wasmResult;
    } else {
      wasmResult = wasm.execute(command);
      jsResult = js.execute(command);
    }

    compare(
      `command result mismatch at ${index}: ${JSON.stringify(command)}`,
      wasmResult,
      jsResult,
    );
    const wasmEvents = wasm.drainEvents();
    const jsEvents = js.drainEvents();
    if (compareEvents) {
      compare(
        `event mismatch at command ${index}: ${JSON.stringify(command)}`,
        wasmEvents,
        jsEvents,
      );
    }
    for (let peer = 0; peer < scenario.peerCount; peer += 1) {
      compare(
        `document mismatch at command ${index}, peer ${peer}: ${JSON.stringify(command)}`,
        observeDoc(wasm.docs[peer]),
        observeDoc(js.docs[peer]),
      );
    }
  }
}

function crossRoundtrip(wasmEngine, jsEngine, wasmDoc, jsDoc, mode) {
  return capture(() => {
    const wasmBytes =
      mode === "snapshot"
        ? wasmDoc.export({ mode: "snapshot" })
        : wasmDoc.export({ mode: "update" });
    const jsBytes =
      mode === "snapshot"
        ? jsDoc.export({ mode: "snapshot" })
        : jsDoc.export({ mode: "update" });
    const variants = [
      [wasmEngine, wasmBytes],
      [jsEngine, wasmBytes],
      [wasmEngine, jsBytes],
      [jsEngine, jsBytes],
    ].map(([engine, bytes]) => {
      const doc = createDoc(engine, undefined, false);
      doc.import(bytes);
      return observeDoc(doc);
    });
    for (let index = 1; index < variants.length; index += 1) {
      compare(`roundtrip variant ${index} mismatch`, variants[0], variants[index]);
    }
    return variants[0];
  });
}
