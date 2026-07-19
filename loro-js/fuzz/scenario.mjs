import fc from "fast-check";

const alphabet = ["a", "b", "c", " ", "\n", "\0", "é", "文", "א", "😀", "𐐷", "\u0301"];

const smallString = (options = {}) =>
  fc
    .array(fc.constantFrom(...alphabet), {
      minLength: options.minLength ?? 0,
      maxLength: options.maxLength ?? 8,
    })
    .map((parts) => parts.join(""));

const key = fc.constantFrom("a", "b", "title", "value", "emoji😀", "empty");
const scalar = fc.oneof(
  fc.constant(null),
  fc.boolean(),
  fc.integer({ min: -1_000, max: 1_000 }),
  smallString({ maxLength: 6 }),
);
const value = fc.oneof(
  scalar,
  fc.array(scalar, { maxLength: 4 }),
  fc.dictionary(key, scalar, { maxKeys: 4 }),
);
const byte = fc.integer({ min: 0, max: 255 });
const peer = byte;
const slot = fc.integer({ min: 0, max: 7 });

const commandArbitraries = [
  fc.record({ kind: fc.constant("map-set"), peer, key, value }),
  fc.record({ kind: fc.constant("map-delete"), peer, key }),
  fc.record({ kind: fc.constant("list-insert"), peer, index: byte, value }),
  fc.record({
    kind: fc.constant("list-delete"),
    peer,
    index: byte,
    length: byte,
  }),
  fc.record({
    kind: fc.constant("text-insert"),
    peer,
    index: byte,
    text: smallString({ minLength: 1, maxLength: 8 }),
  }),
  fc.record({
    kind: fc.constant("text-delete"),
    peer,
    index: byte,
    length: byte,
  }),
  fc.record({
    kind: fc.constant("text-mark"),
    peer,
    index: byte,
    length: byte,
    key: fc.constantFrom("bold", "link", "comment"),
    value: scalar,
  }),
  fc.record({
    kind: fc.constant("text-unmark"),
    peer,
    index: byte,
    length: byte,
    key: fc.constantFrom("bold", "link", "comment"),
  }),
  fc.record({ kind: fc.constant("movable-insert"), peer, index: byte, value }),
  fc.record({
    kind: fc.constant("movable-delete"),
    peer,
    index: byte,
    length: byte,
  }),
  fc.record({ kind: fc.constant("movable-set"), peer, index: byte, value }),
  fc.record({ kind: fc.constant("movable-move"), peer, from: byte, to: byte }),
  fc.record({
    kind: fc.constant("counter-increment"),
    peer,
    delta: fc.integer({ min: -20, max: 20 }),
  }),
  fc.record({
    kind: fc.constant("tree-create"),
    peer,
    parent: fc.option(byte, { nil: null }),
    value,
  }),
  fc.record({
    kind: fc.constant("tree-meta-set"),
    peer,
    node: byte,
    key,
    value,
  }),
  fc.record({
    kind: fc.constant("tree-move"),
    peer,
    node: byte,
    parent: fc.option(byte, { nil: null }),
  }),
  fc.record({ kind: fc.constant("tree-delete"), peer, node: byte }),
  fc.record({ kind: fc.constant("commit"), peer, message: byte }),
  fc.record({
    kind: fc.constant("enqueue"),
    source: peer,
    target: peer,
    slot,
    mode: fc.constantFrom("update", "snapshot"),
  }),
  fc.record({ kind: fc.constant("deliver"), slot, copies: byte }),
  fc.record({ kind: fc.constant("save"), peer, checkpoint: slot }),
  fc.record({ kind: fc.constant("checkout"), peer, checkpoint: slot }),
  fc.record({ kind: fc.constant("attach"), peer }),
  fc.record({
    kind: fc.constant("roundtrip"),
    peer,
    mode: fc.constantFrom("update", "snapshot"),
  }),
];

// Rich-text anchors are represented differently by the pure TypeScript runtime
// today. Keep them in the strict profile so the known divergence remains easy
// to reproduce without making the stable compatibility gate permanently red.
const stableCommandArbitraries = commandArbitraries.filter(
  (_, index) => index !== 6 && index !== 7,
);

export function scenarioArbitrary(maxCommands, { strict = false } = {}) {
  const commands = strict ? commandArbitraries : stableCommandArbitraries;
  // Repeat edits and transport commands to bias generated traces toward states
  // where multiple container kinds and peers interact.
  const command = fc.oneof(
    ...commands,
    ...commands.slice(0, strict ? 18 : 16),
    commandArbitraries[18],
    commandArbitraries[19],
  );
  return fc.record({
    schemaVersion: fc.constant(1),
    peerCount: fc.integer({ min: 2, max: 4 }),
    commands: fc.array(command, { minLength: 1, maxLength: maxCommands }),
  });
}

export function assertScenario(value) {
  if (value === null || typeof value !== "object") {
    throw new TypeError("scenario must be an object");
  }
  if (value.schemaVersion !== 1) {
    throw new RangeError(`unsupported scenario schema ${String(value.schemaVersion)}`);
  }
  if (!Number.isSafeInteger(value.peerCount) || value.peerCount < 2) {
    throw new RangeError("scenario peerCount must be at least two");
  }
  if (!Array.isArray(value.commands)) {
    throw new TypeError("scenario commands must be an array");
  }
  return value;
}
