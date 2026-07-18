import { PostcardReader, PostcardWriter } from "../codec/postcard";
import type { EncodedLoroValue } from "../codec/types";
import { readPostcardValue, writePostcardValue } from "../codec/value";
import { formatContainerId, isContainerId, parseContainerId, parsePeerId } from "./ids";
import type { PeerID, PeerIdInput, Value } from "./types";

export type AwarenessListener = (
  update: { updated: PeerID[]; added: PeerID[]; removed: PeerID[] },
  origin: string,
) => void;

export interface EphemeralStoreEvent {
  readonly by: "local" | "import" | "timeout";
  readonly added: string[];
  readonly updated: string[];
  readonly removed: string[];
}

export type EphemeralListener = (event: EphemeralStoreEvent) => void;
export type EphemeralLocalListener = (bytes: Uint8Array) => void;

interface AwarenessState {
  readonly value: EncodedLoroValue;
  readonly counter: number;
  readonly timestamp: number;
}

interface EncodedAwarenessState {
  readonly peer: bigint;
  readonly counter: number;
  readonly value: EncodedLoroValue;
}

/** The low-level awareness class exported by `loro-crdt`. */
export class AwarenessWasm<T extends Value = Value> {
  readonly #peer: bigint;
  readonly #timeout: number;
  readonly #states = new Map<bigint, AwarenessState>();

  constructor(peer: PeerIdInput, timeout: number) {
    this.#peer = parsePeerId(peer);
    this.#timeout = assertTimeout(timeout);
  }

  free(): void {}

  encode(peers: readonly PeerIdInput[]): Uint8Array {
    const now = Date.now();
    const states: EncodedAwarenessState[] = [];
    for (const peerInput of peers) {
      const peer = parsePeerId(peerInput);
      const state = this.#states.get(peer);
      if (state === undefined || now - state.timestamp > this.#timeout) continue;
      states.push({ peer, counter: state.counter, value: state.value });
    }
    return encodeAwarenessStates(states);
  }

  encodeAll(): Uint8Array {
    return this.encode(this.peers());
  }

  apply(bytes: Uint8Array): { updated: PeerID[]; added: PeerID[] } {
    let decoded: EncodedAwarenessState[];
    try {
      decoded = decodeAwarenessStates(bytes);
    } catch (error) {
      throw new Error(`Failed to decode awareness data: ${errorMessage(error)}`);
    }

    const now = Date.now();
    const updated: PeerID[] = [];
    const added: PeerID[] = [];
    for (const state of decoded) {
      const current = this.#states.get(state.peer);
      if (
        state.peer === this.#peer ||
        (current !== undefined && current.counter >= state.counter)
      ) {
        continue;
      }
      this.#states.set(state.peer, {
        value: state.value,
        counter: state.counter,
        timestamp: now,
      });
      (current === undefined ? added : updated).push(peerString(state.peer));
    }
    return { updated, added };
  }

  setLocalState(value: T): void {
    const current = this.#states.get(this.#peer);
    this.#states.set(this.#peer, {
      value: valueToEncoded(value),
      counter: (current?.counter ?? 0) + 1,
      timestamp: Date.now(),
    });
  }

  peer(): PeerID {
    return peerString(this.#peer);
  }

  getAllStates(): Record<PeerID, T> {
    const output: Record<PeerID, T> = {};
    for (const [peer, state] of this.#states) {
      output[peerString(peer)] = encodedToValue(state.value) as T;
    }
    return output;
  }

  getState(peer: PeerIdInput): T | undefined {
    const state = this.#states.get(parsePeerId(peer));
    return state === undefined ? undefined : (encodedToValue(state.value) as T);
  }

  getTimestamp(peer: PeerIdInput): number | undefined {
    return this.#states.get(parsePeerId(peer))?.timestamp;
  }

  removeOutdated(): PeerID[] {
    const now = Date.now();
    const removed: PeerID[] = [];
    for (const peer of this.orderedPeers()) {
      const state = this.#states.get(peer)!;
      if (now - state.timestamp <= this.#timeout) continue;
      this.#states.delete(peer);
      removed.push(peerString(peer));
    }
    return removed;
  }

  length(): number {
    return this.#states.size;
  }

  isEmpty(): boolean {
    return this.#states.size === 0;
  }

  peers(): PeerID[] {
    return this.orderedPeers().map(peerString);
  }

  private orderedPeers(): bigint[] {
    const peers: bigint[] = [];
    if (this.#states.has(this.#peer)) peers.push(this.#peer);
    for (const peer of this.#states.keys()) {
      if (peer !== this.#peer) peers.push(peer);
    }
    return peers;
  }
}

/** @deprecated Use `EphemeralStore` for new code. */
export class Awareness<T extends Value = Value> {
  readonly inner: AwarenessWasm<T>;
  readonly #peer: PeerID;
  readonly #timeout: number;
  readonly #listeners = new Set<AwarenessListener>();
  #timer: ReturnType<typeof setInterval> | undefined;

  constructor(peer: PeerIdInput, timeout = 30_000) {
    this.inner = new AwarenessWasm<T>(peer, timeout);
    this.#peer = this.inner.peer();
    this.#timeout = timeout;
  }

  apply(bytes: Uint8Array, origin = "remote"): void {
    const { updated, added } = this.inner.apply(bytes);
    for (const listener of this.#listeners) {
      listener({ updated, added, removed: [] }, origin);
    }
    this.startTimerIfNotEmpty();
  }

  setLocalState(state: T): void {
    const wasEmpty = this.inner.getState(this.#peer) === undefined;
    this.inner.setLocalState(state);
    const peer = this.inner.peer();
    for (const listener of this.#listeners) {
      listener(
        wasEmpty
          ? { updated: [], added: [peer], removed: [] }
          : { updated: [peer], added: [], removed: [] },
        "local",
      );
    }
    this.startTimerIfNotEmpty();
  }

  getLocalState(): T | undefined {
    return this.inner.getState(this.#peer);
  }

  getAllStates(): Record<PeerID, T> {
    return this.inner.getAllStates();
  }

  encode(peers: readonly PeerIdInput[]): Uint8Array {
    return this.inner.encode(peers);
  }

  encodeAll(): Uint8Array {
    return this.inner.encodeAll();
  }

  addListener(listener: AwarenessListener): void {
    this.#listeners.add(listener);
  }

  removeListener(listener: AwarenessListener): void {
    this.#listeners.delete(listener);
  }

  peers(): PeerID[] {
    return this.inner.peers();
  }

  destroy(): void {
    if (this.#timer !== undefined) clearInterval(this.#timer);
    this.#timer = undefined;
    this.#listeners.clear();
  }

  private startTimerIfNotEmpty(): void {
    if (this.inner.isEmpty() || this.#timer !== undefined) return;
    this.#timer = setInterval(() => {
      const removed = this.inner.removeOutdated();
      if (removed.length > 0) {
        for (const listener of this.#listeners) {
          listener({ updated: [], added: [], removed }, "timeout");
        }
      }
      if (!this.inner.isEmpty()) return;
      clearInterval(this.#timer);
      this.#timer = undefined;
    }, this.#timeout / 2);
  }
}

interface EphemeralState {
  readonly value: EncodedLoroValue | undefined;
  readonly timestamp: number;
}

interface EncodedEphemeralState extends EphemeralState {
  readonly key: string;
}

/** The low-level ephemeral key-value store exported by `loro-crdt`. */
export class EphemeralStoreWasm<T extends Value = Value> {
  readonly #timeout: number;
  readonly #states = new Map<string, EphemeralState>();
  readonly #listeners = new Set<EphemeralListener>();
  readonly #localListeners = new Set<EphemeralLocalListener>();
  readonly #pendingEvents: EphemeralStoreEvent[] = [];
  readonly #pendingLocalUpdates: Uint8Array[] = [];
  // Adds made from inside a dispatch are queued and flushed after that
  // event's dispatch, preserving the former per-event snapshot semantics
  // without copying the listener set per emission. Removals apply to the
  // live set immediately; Set iteration skips not-yet-visited removals,
  // which matches the old `has` guard.
  readonly #queuedListeners: EphemeralListener[] = [];
  readonly #queuedLocalListeners: EphemeralLocalListener[] = [];
  #deferringListeners = false;
  #deferringLocalListeners = false;
  #emittingEvents = false;
  #emittingLocalUpdates = false;

  constructor(timeout: number) {
    this.#timeout = assertTimeout(timeout);
  }

  free(): void {}

  set(key: string, value: T): void {
    this.setLocal(key, valueToEncoded(value));
  }

  delete(key: string): void {
    this.setLocal(key, undefined);
  }

  get(key: string): T | undefined {
    const value = this.#states.get(key)?.value;
    return value === undefined ? undefined : (encodedToValue(value) as T);
  }

  getAllStates(): Record<string, T> {
    const output: Record<string, T> = {};
    for (const [key, state] of this.#states) {
      if (state.value !== undefined) output[key] = encodedToValue(state.value) as T;
    }
    return output;
  }

  subscribeLocalUpdates(listener: EphemeralLocalListener): () => void {
    if (this.#deferringLocalListeners) this.#queuedLocalListeners.push(listener);
    else this.#localListeners.add(listener);
    return () => {
      this.#localListeners.delete(listener);
      const queued = this.#queuedLocalListeners.indexOf(listener);
      if (queued !== -1) this.#queuedLocalListeners.splice(queued, 1);
    };
  }

  subscribe(listener: EphemeralListener): () => void {
    if (this.#deferringListeners) this.#queuedListeners.push(listener);
    else this.#listeners.add(listener);
    return () => {
      this.#listeners.delete(listener);
      const queued = this.#queuedListeners.indexOf(listener);
      if (queued !== -1) this.#queuedListeners.splice(queued, 1);
    };
  }

  encode(key: string): Uint8Array {
    const state = this.#states.get(key);
    if (state === undefined) return encodeEphemeralStates([]);
    if (Date.now() - state.timestamp > this.#timeout) return new Uint8Array();
    return encodeEphemeralStates([{ key, ...state }]);
  }

  encodeAll(): Uint8Array {
    const now = Date.now();
    const states: EncodedEphemeralState[] = [];
    for (const [key, state] of this.#states) {
      if (now - state.timestamp <= this.#timeout) states.push({ key, ...state });
    }
    return encodeEphemeralStates(states);
  }

  apply(bytes: Uint8Array): void {
    let decoded: EncodedEphemeralState[];
    try {
      decoded = decodeEphemeralStates(bytes);
    } catch (error) {
      throw new Error(`Failed to decode data: ${errorMessage(error)}`);
    }

    const added: string[] = [];
    const updated: string[] = [];
    const removed: string[] = [];
    const now = Date.now();
    for (const state of decoded) {
      if (now - state.timestamp > this.#timeout) continue;
      const old = this.#states.get(state.key);
      if (old !== undefined && old.timestamp >= state.timestamp) continue;
      this.#states.set(state.key, { value: state.value, timestamp: state.timestamp });
      if (old !== undefined && state.value !== undefined) updated.push(state.key);
      else if (old === undefined && state.value !== undefined) added.push(state.key);
      else if (old !== undefined && state.value === undefined) removed.push(state.key);
    }
    this.emit({ by: "import", added, updated, removed });
  }

  removeOutdated(): void {
    const now = Date.now();
    const removed: string[] = [];
    for (const [key, state] of this.#states) {
      if (now - state.timestamp <= this.#timeout) continue;
      this.#states.delete(key);
      if (state.value !== undefined) removed.push(key);
    }
    this.emit({ by: "timeout", added: [], updated: [], removed });
  }

  isEmpty(): boolean {
    return this.#states.size === 0;
  }

  keys(): string[] {
    const output: string[] = [];
    for (const [key, state] of this.#states) {
      if (state.value !== undefined) output.push(key);
    }
    return output;
  }

  private setLocal(key: string, value: EncodedLoroValue | undefined): void {
    const old = this.#states.get(key);
    this.#states.set(key, { value, timestamp: Date.now() });
    const bytes = this.encode(key);
    this.emitLocalUpdate(bytes);

    const isDelete = value === undefined;
    this.emit({
      by: "local",
      added: old === undefined && !isDelete ? [key] : [],
      updated: old !== undefined && !isDelete ? [key] : [],
      removed: old !== undefined && isDelete ? [key] : [],
    });
  }

  private emit(event: EphemeralStoreEvent): void {
    if (this.#emittingEvents) {
      this.#pendingEvents.push(event);
      return;
    }

    this.#emittingEvents = true;
    const pending = [event];
    try {
      while (pending.length > 0) {
        const current = pending.pop()!;
        this.#deferringListeners = true;
        try {
          for (const listener of this.#listeners) listener(current);
        } finally {
          this.#deferringListeners = false;
          for (const listener of this.#queuedListeners) this.#listeners.add(listener);
          this.#queuedListeners.length = 0;
        }
        pending.push(...this.#pendingEvents.splice(0));
      }
    } finally {
      this.#emittingEvents = false;
      this.#pendingEvents.length = 0;
    }
  }

  private emitLocalUpdate(bytes: Uint8Array): void {
    if (this.#emittingLocalUpdates) {
      this.#pendingLocalUpdates.push(bytes);
      return;
    }

    this.#emittingLocalUpdates = true;
    const pending = [bytes];
    try {
      while (pending.length > 0) {
        const current = pending.pop()!;
        this.#deferringLocalListeners = true;
        try {
          for (const listener of this.#localListeners) listener(current);
        } finally {
          this.#deferringLocalListeners = false;
          for (const listener of this.#queuedLocalListeners) {
            this.#localListeners.add(listener);
          }
          this.#queuedLocalListeners.length = 0;
        }
        pending.push(...this.#pendingLocalUpdates.splice(0));
      }
    } finally {
      this.#emittingLocalUpdates = false;
      this.#pendingLocalUpdates.length = 0;
    }
  }
}

/** A typed, automatically expiring wrapper around `EphemeralStoreWasm`. */
export class EphemeralStore<T extends Record<string, Value> = Record<string, Value>> {
  readonly inner: EphemeralStoreWasm;
  readonly #timeout: number;
  #timer: ReturnType<typeof setInterval> | undefined;

  constructor(timeout = 30_000) {
    this.inner = new EphemeralStoreWasm(timeout);
    this.#timeout = timeout;
  }

  apply(bytes: Uint8Array): void {
    this.inner.apply(bytes);
    this.startTimerIfNotEmpty();
  }

  set<K extends keyof T>(key: K, value: T[K]): void {
    this.inner.set(key as string, value);
    this.startTimerIfNotEmpty();
  }

  delete<K extends keyof T>(key: K): void {
    this.inner.delete(key as string);
  }

  get<K extends keyof T>(key: K): T[K] | undefined {
    return this.inner.get(key as string) as T[K] | undefined;
  }

  getAllStates(): Partial<T> {
    return this.inner.getAllStates() as Partial<T>;
  }

  encode<K extends keyof T>(key: K): Uint8Array {
    return this.inner.encode(key as string);
  }

  encodeAll(): Uint8Array {
    return this.inner.encodeAll();
  }

  keys(): string[] {
    return this.inner.keys();
  }

  destroy(): void {
    if (this.#timer !== undefined) clearInterval(this.#timer);
    this.#timer = undefined;
  }

  subscribe(listener: EphemeralListener): () => void {
    return this.inner.subscribe(listener);
  }

  subscribeLocalUpdates(listener: EphemeralLocalListener): () => void {
    return this.inner.subscribeLocalUpdates(listener);
  }

  private startTimerIfNotEmpty(): void {
    if (this.inner.isEmpty() || this.#timer !== undefined) return;
    this.#timer = setInterval(() => {
      this.inner.removeOutdated();
      if (!this.inner.isEmpty()) return;
      clearInterval(this.#timer);
      this.#timer = undefined;
    }, this.#timeout / 2);
  }
}

function encodeAwarenessStates(states: readonly EncodedAwarenessState[]): Uint8Array {
  const writer = new PostcardWriter();
  writer.writeUsize(states.length);
  for (const state of states) {
    writer.writeU64(state.peer);
    writer.writeI32(state.counter);
    writePostcardValue(writer, state.value);
  }
  return writer.toUint8Array();
}

function decodeAwarenessStates(bytes: Uint8Array): EncodedAwarenessState[] {
  const reader = new PostcardReader(bytes);
  const states = reader.readArray((input) => ({
    peer: input.readU64(),
    counter: input.readI32(),
    value: readPostcardValue(input),
  }));
  reader.assertEnd();
  return states;
}

function encodeEphemeralStates(states: readonly EncodedEphemeralState[]): Uint8Array {
  const writer = new PostcardWriter();
  writer.writeUsize(states.length);
  for (const state of states) {
    writer.writeString(state.key);
    writer.writeOption(state.value, writePostcardValue);
    writer.writeI64(BigInt(state.timestamp));
  }
  return writer.toUint8Array();
}

function decodeEphemeralStates(bytes: Uint8Array): EncodedEphemeralState[] {
  const reader = new PostcardReader(bytes);
  const states = reader.readArray((input) => ({
    key: input.readString(),
    value: input.readOption(readPostcardValue),
    timestamp: Number(input.readI64()),
  }));
  reader.assertEnd();
  return states;
}

function valueToEncoded(value: unknown, depth = 0): EncodedLoroValue {
  if (depth > 512) throw new RangeError("LoroValue nesting depth exceeded");
  if (value === null || value === undefined) return { type: "null" };
  if (typeof value === "boolean") return { type: "bool", value };
  if (typeof value === "number") {
    return Number.isInteger(value) && value >= -(2 ** 53) && value <= 2 ** 53
      ? { type: "i64", value: BigInt(value) }
      : { type: "double", value };
  }
  if (typeof value === "string") {
    return isContainerId(value)
      ? { type: "container", value: parseContainerId(value) }
      : { type: "string", value };
  }
  if (value instanceof Uint8Array) return { type: "binary", value: value.slice() };
  if (Array.isArray(value)) {
    return { type: "list", value: value.map((item) => valueToEncoded(item, depth + 1)) };
  }
  if (value instanceof Map) {
    return {
      type: "map",
      value: [...value].map(([key, item]) => {
        if (typeof key !== "string") throw new TypeError("Map keys must be strings");
        return [key, valueToEncoded(item, depth + 1)] as const;
      }),
    };
  }
  if (typeof value === "object") {
    return {
      type: "map",
      value: Object.entries(value).map(
        ([key, item]) => [key, valueToEncoded(item, depth + 1)] as const,
      ),
    };
  }
  throw new TypeError(`unsupported LoroValue type: ${typeof value}`);
}

function encodedToValue(value: EncodedLoroValue): Value {
  switch (value.type) {
    case "null":
      return null;
    case "bool":
    case "double":
    case "string":
      return value.value;
    case "i64":
      return Number(value.value);
    case "binary":
      return value.value.slice();
    case "list":
      return value.value.map(encodedToValue);
    case "map":
      return Object.fromEntries(
        value.value.map(([key, item]) => [key, encodedToValue(item)]),
      );
    case "container":
      return formatContainerId(value.value);
  }
}

function assertTimeout(timeout: number): number {
  if (!Number.isFinite(timeout)) throw new TypeError("timeout must be finite");
  return timeout;
}

function peerString(peer: bigint): PeerID {
  return peer.toString() as PeerID;
}

function errorMessage(error: unknown): string {
  return error instanceof Error ? error.message : String(error);
}
