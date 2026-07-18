import type { JsonOp, JsonSchema, JsonValue, PeerID } from "./types";

export type VersionRange = Readonly<Record<string, readonly [number, number]>>;

export function redactJsonUpdates(
  input: JsonSchema | string,
  versionRange: VersionRange,
): JsonSchema {
  const parsed = typeof input === "string" ? (JSON.parse(input) as JsonSchema) : input;
  const schema = cloneJsonUpdateValue(parsed) as JsonSchema;
  for (const change of schema.changes) {
    const peerToken = change.id.slice(change.id.indexOf("@") + 1);
    const peer =
      schema.peers === null ? peerToken : (schema.peers[Number(peerToken)] ?? peerToken);
    const range = versionRange[peer];
    if (range === undefined) continue;
    for (const operation of change.ops) redactOperation(operation, range);
  }
  return schema;
}

function redactOperation(operation: JsonOp, range: readonly [number, number]): void {
  const content = operation.content as unknown as Record<string, unknown> & {
    type: string;
  };
  if (content.type === "insert" && typeof content.text === "string") {
    const characters = Array.from(content.text);
    content.text = characters
      .map((character, index) =>
        inRange(operation.counter + index, range) ? "�" : character,
      )
      .join("");
    return;
  }
  if (content.type === "insert" && Array.isArray(content.value)) {
    content.value = content.value.map((value, index) =>
      inRange(operation.counter + index, range) ? redactValue(value) : value,
    );
    return;
  }
  if (!inRange(operation.counter, range)) return;
  if (content.type === "insert" && typeof content.key === "string") {
    content.value = redactValue(content.value);
  } else if (content.type === "set") {
    content.value = redactValue(content.value);
  } else if (content.type === "mark") {
    content.style_value = null;
  }
}

function redactValue(value: unknown): JsonValue {
  return typeof value === "string" && value.startsWith("🦜:")
    ? (value as JsonValue)
    : null;
}

function inRange(counter: number, range: readonly [number, number]): boolean {
  return counter >= range[0] && counter < range[1];
}

function cloneJsonUpdateValue(value: unknown): unknown {
  if (value instanceof Uint8Array) return value.slice();
  if (Array.isArray(value)) return value.map(cloneJsonUpdateValue);
  if (typeof value === "object" && value !== null) {
    return Object.fromEntries(
      Object.entries(value).map(([key, item]) => [key, cloneJsonUpdateValue(item)]),
    );
  }
  return value;
}

export function jsonUpdatePeer(schema: JsonSchema, compressedPeer: string): PeerID {
  return (schema.peers?.[Number(compressedPeer)] ?? compressedPeer) as PeerID;
}
