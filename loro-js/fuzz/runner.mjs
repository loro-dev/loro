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

function releaseResource(resource) {
  resource?.free?.();
}

function releaseResources(resources) {
  for (const resource of resources) releaseResource(resource);
}

function usingResource(resource, action) {
  try {
    return action(resource);
  } finally {
    releaseResource(resource);
  }
}

function canonicalVersionAndRelease(version) {
  return usingResource(version, canonicalVersion);
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

function normalizeEventBatch(batch) {
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
    const deletedTreeNodes = collectDeletedTreeNodes(normalized.events);
    for (const event of normalized.events) {
      if (Array.isArray(event.diff?.diff)) {
        event.diff.diff = trimPlainTrailingRetains(
          canonicalizeDeltaOrder(coalesceDelta(event.diff.diff)),
        );
      }
      if (event.diff?.type === "tree" && Array.isArray(event.diff.diff)) {
        event.diff.diff = removeTransientTreeActions(event.diff.diff);
      }
    }
    normalized.events = normalized.events.filter(
      (event) =>
        eventHasEffect(event) && !targetsTreeNodeDeletedInBatch(event, deletedTreeNodes),
    );
    normalized.events.sort((left, right) =>
      JSON.stringify([left.target, left.path]).localeCompare(
        JSON.stringify([right.target, right.path]),
      ),
    );
  }
  return canonicalize(normalized);
}

function collectDeletedTreeNodes(events) {
  const deleted = new Set();
  for (const event of events) {
    if (event.diff?.type !== "tree" || !Array.isArray(event.diff.diff)) continue;
    const root = event.path?.[0];
    if (typeof root !== "string") continue;
    for (const action of event.diff.diff) {
      if (action.action === "delete" && typeof action.target === "string") {
        deleted.add(JSON.stringify([root, action.target]));
      }
    }
  }
  return deleted;
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
  const actionsByTarget = new Map();
  for (const action of actions) {
    let targetActions = actionsByTarget.get(action.target);
    if (targetActions === undefined) {
      targetActions = [];
      actionsByTarget.set(action.target, targetActions);
    }
    targetActions.push(action);
  }
  const finalParents = new Map();
  const finalDeleted = new Set();
  for (const action of actions) {
    if (action.action === "delete") {
      finalDeleted.add(action.target);
    } else {
      finalDeleted.delete(action.target);
      finalParents.set(action.target, normalizeTreeParent(action.parent));
    }
  }
  const endsDeleted = (target, visiting = new Set()) => {
    if (finalDeleted.has(target)) return true;
    const parent = finalParents.get(target);
    if (parent === undefined || parent === null || visiting.has(parent)) return false;
    visiting.add(parent);
    return endsDeleted(parent, visiting);
  };
  const transientTargets = new Set();
  for (const [target, targetActions] of actionsByTarget) {
    if (targetActions[0]?.action === "create" && endsDeleted(target)) {
      transientTargets.add(target);
    }
  }

  const positions = new Map();
  const countBefore = (parent, index, target) => {
    let count = 0;
    for (const [candidate, position] of positions) {
      if (
        candidate !== target &&
        transientTargets.has(candidate) &&
        position.parent === parent &&
        position.index < index
      ) {
        count += 1;
      }
    }
    return count;
  };
  const removeAt = (parent, index, target) => {
    positions.delete(target);
    for (const position of positions.values()) {
      if (position.parent === parent && position.index > index) position.index -= 1;
    }
  };
  const insertAt = (parent, index, target) => {
    for (const [candidate, position] of positions) {
      if (candidate !== target && position.parent === parent && position.index >= index) {
        position.index += 1;
      }
    }
    positions.set(target, { parent, index });
  };

  const output = [];
  for (const action of actions) {
    const transient = transientTargets.has(action.target);
    const normalized = structuredClone(action);
    if (action.action === "create") {
      const parent = normalizeTreeParent(action.parent);
      if (!transient) {
        normalized.index -= countBefore(parent, action.index, action.target);
      }
      insertAt(parent, action.index, action.target);
    } else if (action.action === "delete") {
      const oldParent = normalizeTreeParent(action.oldParent);
      if (!transient) {
        normalized.oldIndex -= countBefore(oldParent, action.oldIndex, action.target);
      }
      const position = positions.get(action.target) ?? {
        parent: oldParent,
        index: action.oldIndex,
      };
      removeAt(position.parent, position.index, action.target);
    } else if (action.action === "move") {
      const oldParent = normalizeTreeParent(action.oldParent);
      const parent = normalizeTreeParent(action.parent);
      if (!transient) {
        normalized.oldIndex -= countBefore(oldParent, action.oldIndex, action.target);
      }
      const previous = positions.get(action.target) ?? {
        parent: oldParent,
        index: action.oldIndex,
      };
      removeAt(previous.parent, previous.index, action.target);
      if (!transient) {
        normalized.index -= countBefore(parent, action.index, action.target);
      }
      insertAt(parent, action.index, action.target);
    }
    if (!transient) output.push(normalized);
  }
  const hasComposedTarget = output.some((action, index) =>
    output.slice(index + 1).some((candidate) => candidate.target === action.target),
  );
  return coalesceTreeActions(
    output,
    hasComposedTarget ? positions : undefined,
    transientTargets,
  );
}

function coalesceTreeActions(actions, finalPositions, transientTargets) {
  const actionsByTarget = new Map();
  for (const action of actions) {
    let targetActions = actionsByTarget.get(action.target);
    if (targetActions === undefined) {
      targetActions = [];
      actionsByTarget.set(action.target, targetActions);
    }
    targetActions.push(action);
  }
  const departures = [];
  if (finalPositions === undefined) {
    for (const [target, targetActions] of actionsByTarget) {
      const first = targetActions[0];
      if (first.action === "create") continue;
      const last = targetActions.at(-1);
      const oldParent = normalizeTreeParent(first.oldParent);
      const parent =
        last.action === "delete" ? undefined : normalizeTreeParent(last.parent);
      if (parent !== oldParent) {
        departures.push({ target, parent: oldParent, index: first.oldIndex });
      }
    }
  }
  const finalIndex = (target, parent, fallback) => {
    const position = finalPositions?.get(target);
    if (position === undefined) {
      const removedBefore = departures.filter(
        (departure) =>
          departure.target !== target &&
          departure.parent === parent &&
          departure.index < fallback,
      ).length;
      return fallback - removedBefore;
    }
    let removedBefore = 0;
    for (const [candidate, candidatePosition] of finalPositions) {
      if (
        transientTargets.has(candidate) &&
        candidatePosition.parent === position.parent &&
        candidatePosition.index < position.index
      ) {
        removedBefore += 1;
      }
    }
    return position.index - removedBefore;
  };
  const output = [];
  for (const [target, targetActions] of actionsByTarget) {
    const first = targetActions[0];
    const last = targetActions.at(-1);
    const existedBefore = first.action !== "create";
    const existsAfter = last.action !== "delete";
    if (!existedBefore && !existsAfter) continue;
    if (!existedBefore) {
      output.push({
        target,
        action: "create",
        parent: last.parent,
        index: finalIndex(target, normalizeTreeParent(last.parent), last.index),
        fractional_index: last.fractional_index,
      });
      continue;
    }
    const oldParent = normalizeTreeParent(first.oldParent);
    const oldIndex = first.oldIndex;
    if (!existsAfter) {
      output.push({ target, action: "delete", oldParent, oldIndex });
      continue;
    }
    const parent = normalizeTreeParent(last.parent);
    const index = finalIndex(target, parent, last.index);
    if (parent === oldParent && index === oldIndex) continue;
    output.push({
      target,
      action: "move",
      parent,
      index,
      fractional_index: last.fractional_index,
      oldParent,
      oldIndex,
    });
  }
  return output.sort((left, right) => left.target.localeCompare(right.target));
}

function normalizeTreeParent(parent) {
  return parent ?? null;
}

function targetsTreeNodeDeletedInBatch(event, deletedTreeNodes) {
  const [root, node] = event.path ?? [];
  return (
    typeof root === "string" &&
    typeof node === "string" &&
    deletedTreeNodes.has(JSON.stringify([root, node]))
  );
}

function eventHasEffect(event) {
  const diff = event.diff;
  if (Array.isArray(diff?.diff)) {
    if (diff.type === "list" || diff.type === "text") {
      return diff.diff.some(
        (operation) =>
          operation.insert !== undefined ||
          operation.delete !== undefined ||
          (operation.attributes !== undefined &&
            Object.keys(operation.attributes).length > 0),
      );
    }
    return diff.diff.length > 0;
  }
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

function trimPlainTrailingRetains(delta) {
  while (delta.length > 0) {
    const operation = delta.at(-1);
    if (
      typeof operation?.retain !== "number" ||
      (operation.attributes !== undefined && Object.keys(operation.attributes).length > 0)
    ) {
      break;
    }
    delta.pop();
  }
  return delta;
}

export function observeDoc(doc) {
  const pendingLength = doc.getPendingTxnLength();
  const tree = doc.getTree("tree");
  const nodes = tree.getNodes({ withDeleted: true });
  const text = doc.getText("text");
  try {
    const treeNodes = nodes
      .map((node) => node.toJSON())
      .sort((left, right) => String(left.id).localeCompare(String(right.id)));
    const observation = {
      json: doc.toJSON(),
      deepWithId: normalizeDeepWithId(doc.getDeepValueWithID()),
      textDelta: text.toDelta(),
      treeNodes,
      version: canonicalVersionAndRelease(doc.version()),
      frontiers: canonicalFrontiers(doc.frontiers()),
      detached: doc.isDetached(),
      shallow: doc.isShallow(),
      pendingLength,
    };
    if (pendingLength === 0) {
      observation.oplogVersion = canonicalVersionAndRelease(doc.oplogVersion());
      observation.oplogFrontiers = canonicalFrontiers(doc.oplogFrontiers());
      observation.opCount = doc.opCount();
      observation.changeCount = doc.changeCount();
    }
    return canonicalize(observation);
  } finally {
    releaseResources(nodes);
    releaseResource(text);
    releaseResource(tree);
  }
}

export function observeDocForNative(doc) {
  return canonicalize({
    json: doc.toJSON(),
    deepWithId: normalizeDeepWithId(doc.getDeepValueWithID()),
    version: canonicalVersionAndRelease(doc.version()),
    oplogVersion: canonicalVersionAndRelease(doc.oplogVersion()),
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
  let parent = node.parent();
  while (parent !== undefined) {
    if (parent.id === ancestorId) {
      releaseResource(parent);
      return true;
    }
    const next = parent.parent();
    releaseResource(parent);
    parent = next;
  }
  return false;
}

function usingLiveTreeNodes(doc, action) {
  return usingResource(doc.getTree("tree"), (tree) => {
    const nodes = liveTreeNodes(tree);
    try {
      return action(tree, nodes);
    } finally {
      releaseResources(nodes);
    }
  });
}

function applyEdit(doc, command) {
  switch (command.kind) {
    case "map-set":
      return usingResource(doc.getMap("map"), (map) =>
        map.set(command.key, command.value),
      );
    case "map-delete":
      return usingResource(doc.getMap("map"), (map) => {
        const keys = map.keys();
        if (keys.length === 0) return undefined;
        const key = keys.includes(command.key) ? command.key : keys.sort()[0];
        return map.delete(key);
      });
    case "list-insert":
      return usingResource(doc.getList("list"), (list) =>
        list.insert(modulo(command.index, list.length + 1), command.value),
      );
    case "list-delete":
      return usingResource(doc.getList("list"), (list) => {
        if (list.length === 0) return undefined;
        const index = modulo(command.index, list.length);
        const length = 1 + modulo(command.length, list.length - index);
        return list.delete(index, length);
      });
    case "text-insert":
      return usingResource(doc.getText("text"), (text) => {
        const boundaries = utf16Boundaries(text.toString());
        return text.insert(
          boundaries[modulo(command.index, boundaries.length)],
          command.text,
        );
      });
    case "text-delete":
      return usingResource(doc.getText("text"), (text) => {
        const { start, end } = selectedTextRange(
          text,
          command.index,
          command.length,
          false,
        );
        if (start === end) return undefined;
        return text.delete(start, end - start);
      });
    case "text-mark":
      return usingResource(doc.getText("text"), (text) => {
        const { start, end } = selectedTextRange(
          text,
          command.index,
          command.length,
          false,
        );
        if (start === end) return undefined;
        return text.mark({ start, end }, command.key, command.value);
      });
    case "text-unmark":
      return usingResource(doc.getText("text"), (text) => {
        const { start, end } = selectedTextRange(
          text,
          command.index,
          command.length,
          false,
        );
        if (start === end) return undefined;
        return text.unmark({ start, end }, command.key);
      });
    case "movable-insert":
      return usingResource(doc.getMovableList("movable"), (list) =>
        list.insert(modulo(command.index, list.length + 1), command.value),
      );
    case "movable-delete":
      return usingResource(doc.getMovableList("movable"), (list) => {
        if (list.length === 0) return undefined;
        const index = modulo(command.index, list.length);
        const length = 1 + modulo(command.length, list.length - index);
        return list.delete(index, length);
      });
    case "movable-set":
      return usingResource(doc.getMovableList("movable"), (list) => {
        if (list.length === 0) return undefined;
        const index = modulo(command.index, list.length);
        if (
          isDeepStrictEqual(canonicalize(list.get(index)), canonicalize(command.value))
        ) {
          return undefined;
        }
        return list.set(index, command.value);
      });
    case "movable-move":
      return usingResource(doc.getMovableList("movable"), (list) => {
        if (list.length < 2) return undefined;
        return list.move(
          modulo(command.from, list.length),
          modulo(command.to, list.length),
        );
      });
    case "counter-increment":
      return command.delta === 0
        ? undefined
        : usingResource(doc.getCounter("counter"), (counter) =>
            counter.increment(command.delta),
          );
    case "tree-create":
      return usingLiveTreeNodes(doc, (tree, nodes) => {
        const parent =
          command.parent === null || nodes.length === 0
            ? undefined
            : nodes[modulo(command.parent, nodes.length)].id;
        const node = tree.createNode(parent);
        try {
          usingResource(node.data, (data) => data.set("value", command.value));
          return node.id;
        } finally {
          releaseResource(node);
        }
      });
    case "tree-meta-set":
      return usingLiveTreeNodes(doc, (_tree, nodes) => {
        if (nodes.length === 0) return undefined;
        return usingResource(nodes[modulo(command.node, nodes.length)].data, (data) =>
          data.set(command.key, command.value),
        );
      });
    case "tree-move":
      return usingLiveTreeNodes(doc, (tree, nodes) => {
        if (nodes.length === 0) return undefined;
        const node = nodes[modulo(command.node, nodes.length)];
        const candidates = nodes.filter(
          (candidate) =>
            candidate.id !== node.id && !isTreeDescendant(candidate, node.id),
        );
        const parent =
          command.parent === null || candidates.length === 0
            ? undefined
            : candidates[modulo(command.parent, candidates.length)];
        const currentParent = node.parent();
        const currentParentId = currentParent?.id;
        releaseResource(currentParent);
        if (currentParentId === parent?.id) return undefined;
        return tree.move(node.id, parent?.id);
      });
    case "tree-delete":
      return usingLiveTreeNodes(doc, (tree, nodes) => {
        if (nodes.length === 0) return undefined;
        return tree.delete(nodes[modulo(command.node, nodes.length)].id);
      });
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
    releaseResource(doc.getMap("map"));
    releaseResource(doc.getList("list"));
    releaseResource(doc.getText("text"));
    releaseResource(doc.getMovableList("movable"));
    releaseResource(doc.getCounter("counter"));
    usingResource(doc.getTree("tree"), (tree) => tree.enableFractionalIndex(0));
  }
  return doc;
}

function exportFor(doc, target, mode) {
  if (mode === "snapshot") return doc.export({ mode: "snapshot" });
  const from = target.oplogVersion();
  try {
    return doc.export({ mode: "update", from });
  } finally {
    releaseResource(from);
  }
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
        this.eventQueues[peer].push(batch);
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
      try {
        imported.import(bytes);
        return observeDoc(imported);
      } finally {
        releaseResource(imported);
      }
    });
  }

  finalNativeObservations() {
    return this.docs.map(observeDocForNative);
  }

  drainEvents() {
    this.engine.callPendingEvents?.();
    return this.eventQueues.map((queue) =>
      queue
        .splice(0)
        .map(normalizeEventBatch)
        .filter((batch) => batch.events.length > 0),
    );
  }

  dispose() {
    for (const subscription of this.subscriptions) {
      if (typeof subscription === "function") subscription();
      else releaseResource(subscription);
    }
    releaseResources(this.docs);
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
  try {
    const results = [];
    for (const command of scenario.commands) results.push(world.execute(command));
    return {
      observations: world.finalNativeObservations(),
      transportBlobs: world.transportBlobs,
      results: canonicalize(results),
    };
  } finally {
    world.dispose();
  }
}

export function runMalformedImportChecks(wasmEngine, jsEngine) {
  const engines = [
    ["wasm", wasmEngine],
    ["js", jsEngine],
  ];
  let cases = 0;
  for (const [producerName, producer] of engines) {
    const source = createDoc(producer, 0);
    try {
      usingResource(source.getMap("map"), (map) =>
        map.set("payload", { nested: [1, true, "😀"] }),
      );
      usingResource(source.getText("text"), (text) => text.insert(0, "malformed 😀 文"));
      usingResource(source.getList("list"), (list) => list.push("item"));
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
    } finally {
      releaseResource(source);
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
  try {
    usingResource(target.getMap("map"), (map) => map.set("sentinel", "unchanged"));
    usingResource(target.getText("text"), (text) => text.insert(0, "stable"));
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
  } finally {
    releaseResource(target);
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

  try {
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
  } finally {
    wasm.dispose();
    js.dispose();
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
      try {
        doc.import(bytes);
        return observeDoc(doc);
      } finally {
        releaseResource(doc);
      }
    });
    for (let index = 1; index < variants.length; index += 1) {
      compare(`roundtrip variant ${index} mismatch`, variants[0], variants[index]);
    }
    return variants[0];
  });
}
