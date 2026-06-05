export * from "loro-wasm";
export type * from "loro-wasm";
import {
  AwarenessWasm,
  EphemeralStoreWasm,
  PeerID,
  Container,
  ContainerID,
  ContainerType,
  LoroCounter,
  LoroDoc,
  LoroList,
  LoroMap,
  LoroMovableList,
  LoroText,
  LoroTree,
  LoroTreeNode,
  OpId,
  Value,
  AwarenessListener,
  EphemeralListener,
  EphemeralLocalListener,
  UndoManager,
  callPendingEvents,
} from "loro-wasm";

/**
 * @deprecated Please use LoroDoc
 */
export class Loro extends LoroDoc {}

const CONTAINER_TYPES = [
  "Map",
  "Text",
  "List",
  "Tree",
  "MovableList",
  "Counter",
];
const CONTAINER_KIND = Symbol("loro.containerKind");

export function isContainerId(s: string): s is ContainerID {
  return s.startsWith("cid:");
}

/**  Whether the value is a container.
 *
 * # Example
 *
 * ```ts
 * const doc = new LoroDoc();
 * const map = doc.getMap("map");
 * const list = doc.getList("list");
 * const text = doc.getText("text");
 * isContainer(map); // true
 * isContainer(list); // true
 * isContainer(text); // true
 * isContainer(123); // false
 * isContainer("123"); // false
 * isContainer({}); // false
 * ```
 */
export function isContainer(value: any): value is Container {
  if (typeof value !== "object" || value == null) {
    return false;
  }

  if ((value as { [CONTAINER_KIND]?: ContainerType })[CONTAINER_KIND]) {
    return true;
  }

  const kind = value.kind;
  return (
    typeof kind === "function" && CONTAINER_TYPES.includes(kind.call(value))
  );
}

/**  Get the type of a value that may be a container.
 *
 * # Example
 *
 * ```ts
 * const doc = new LoroDoc();
 * const map = doc.getMap("map");
 * const list = doc.getList("list");
 * const text = doc.getText("text");
 * getType(map); // "Map"
 * getType(list); // "List"
 * getType(text); // "Text"
 * getType(123); // "Json"
 * getType("123"); // "Json"
 * getType({}); // "Json"
 * ```
 */
export function getType<T>(
  value: T,
): T extends LoroText
  ? "Text"
  : T extends LoroMap<any>
  ? "Map"
  : T extends LoroTree<any>
  ? "Tree"
  : T extends LoroList<any>
  ? "List"
  : T extends LoroCounter
  ? "Counter"
  : "Json" {
  if (isContainer(value)) {
    return value.kind() as unknown as any;
  }

  return "Json" as any;
}

export function newContainerID(id: OpId, type: ContainerType): ContainerID {
  return `cid:${id.counter}@${id.peer}:${type}`;
}

export function newRootContainerID(
  name: string,
  type: ContainerType,
): ContainerID {
  return `cid:root-${name}:${type}`;
}

/**
 * @deprecated Please use `EphemeralStore` instead.
 *
 * Awareness is a structure that allows to track the ephemeral state of the peers.
 *
 * If we don't receive a state update from a peer within the timeout, we will remove their state.
 * The timeout is in milliseconds. This can be used to handle the offline state of a peer.
 */
export class Awareness<T extends Value = Value> {
  inner: AwarenessWasm<T>;
  private peer: PeerID;
  private timer: number | undefined;
  private timeout: number;
  private listeners: Set<AwarenessListener> = new Set();
  constructor(peer: PeerID, timeout: number = 30000) {
    this.inner = new AwarenessWasm(peer, timeout);
    this.peer = peer;
    this.timeout = timeout;
  }

  apply(bytes: Uint8Array, origin = "remote") {
    const { updated, added } = this.inner.apply(bytes);
    this.listeners.forEach((listener) => {
      listener({ updated, added, removed: [] }, origin);
    });

    this.startTimerIfNotEmpty();
  }

  setLocalState(state: T) {
    const wasEmpty = this.inner.getState(this.peer) == null;
    this.inner.setLocalState(state);
    if (wasEmpty) {
      this.listeners.forEach((listener) => {
        listener(
          { updated: [], added: [this.inner.peer()], removed: [] },
          "local",
        );
      });
    } else {
      this.listeners.forEach((listener) => {
        listener(
          { updated: [this.inner.peer()], added: [], removed: [] },
          "local",
        );
      });
    }

    this.startTimerIfNotEmpty();
  }

  getLocalState(): T | undefined {
    return this.inner.getState(this.peer);
  }

  getAllStates(): Record<PeerID, T> {
    return this.inner.getAllStates();
  }

  encode(peers: PeerID[]): Uint8Array {
    return this.inner.encode(peers);
  }

  encodeAll(): Uint8Array {
    return this.inner.encodeAll();
  }

  addListener(listener: AwarenessListener) {
    this.listeners.add(listener);
  }

  removeListener(listener: AwarenessListener) {
    this.listeners.delete(listener);
  }

  peers(): PeerID[] {
    return this.inner.peers();
  }

  destroy() {
    clearInterval(this.timer);
    this.listeners.clear();
  }

  private startTimerIfNotEmpty() {
    if (this.inner.isEmpty() || this.timer != null) {
      return;
    }

    this.timer = setInterval(() => {
      const removed = this.inner.removeOutdated();
      if (removed.length > 0) {
        this.listeners.forEach((listener) => {
          listener({ updated: [], added: [], removed }, "timeout");
        });
      }
      if (this.inner.isEmpty()) {
        clearInterval(this.timer);
        this.timer = undefined;
      }
    }, this.timeout / 2) as unknown as number;
  }
}

/**
 * EphemeralStore tracks ephemeral key-value state across peers.
 *
 * - Use it for lightweight presence/state like cursors, selections, and UI hints.
 * - Conflict resolution is timestamp-based LWW (Last-Write-Wins) per key.
 * - Timeout unit: milliseconds.
 * - After timeout: keys are considered expired. They are omitted from
 *   `encode(key)`, `encodeAll()` and `getAllStates()`. A periodic cleanup runs
 *   while the store is non-empty and removes expired keys; when removals happen
 *   subscribers receive an event with `by: "timeout"` and the `removed` keys.
 *
 * See: https://loro.dev/docs/tutorial/ephemeral
 *
 * @param timeout Inactivity timeout in milliseconds (default: 30000). If a key
 * doesn't receive updates within this duration, it will expire and be removed
 * on the next cleanup tick.
 *
 * @example
 * ```ts
 * const store = new EphemeralStore();
 * const store2 = new EphemeralStore();
 * // Subscribe to local updates and forward over the wire
 * store.subscribeLocalUpdates((data) => {
 *   store2.apply(data);
 * });
 * // Subscribe to all updates (including removals by timeout)
 * store2.subscribe((event) => {
 *   console.log("event:", event);
 * });
 * // Set a value
 * store.set("key", "value");
 * // Encode the value
 * const encoded = store.encode("key");
 * // Apply the encoded value
 * store2.apply(encoded);
 * ```
 */
export class EphemeralStore<
  T extends Record<string, Value> = Record<string, Value>,
> {
  inner: EphemeralStoreWasm;
  private timer: number | undefined;
  private timeout: number;
  constructor(timeout: number = 30000) {
    this.inner = new EphemeralStoreWasm(timeout);
    this.timeout = timeout;
  }

  apply(bytes: Uint8Array) {
    this.inner.apply(bytes);
    this.startTimerIfNotEmpty();
  }

  set<K extends keyof T>(key: K, value: T[K]) {
    this.inner.set(key as string, value);
    this.startTimerIfNotEmpty();
  }

  delete<K extends keyof T>(key: K) {
    this.inner.delete(key as string);
  }

  get<K extends keyof T>(key: K): T[K] | undefined {
    return this.inner.get(key as string);
  }

  getAllStates(): Partial<T> {
    return this.inner.getAllStates();
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

  destroy() {
    clearInterval(this.timer);
  }

  subscribe(listener: EphemeralListener) {
    return this.inner.subscribe(listener);
  }

  subscribeLocalUpdates(listener: EphemeralLocalListener) {
    return this.inner.subscribeLocalUpdates(listener);
  }

  private startTimerIfNotEmpty() {
    if (this.inner.isEmpty() || this.timer != null) {
      return;
    }

    this.timer = setInterval(() => {
      this.inner.removeOutdated();
      if (this.inner.isEmpty()) {
        clearInterval(this.timer);
        this.timer = undefined;
      }
    }, this.timeout / 2) as unknown as number;
  }
}

LoroDoc.prototype.toJsonWithReplacer = function (
  replacer: (
    key: string | number,
    value: Value | Container,
  ) => Value | Container | undefined,
) {
  const processed = new Set<string>();
  const doc = this;
  const m = (key: string | number, value: Value): Value | undefined => {
    if (typeof value === "string") {
      if (isContainerId(value) && !processed.has(value)) {
        processed.add(value);
        const container = doc.getContainerById(value);
        if (container == null) {
          throw new Error(`ContainerID not found: ${value}`);
        }

        const ans = replacer(key, container);
        if (ans === container) {
          const ans = container.getShallowValue();
          if (typeof ans === "object") {
            return run(ans as any);
          }

          return ans;
        }

        if (isContainer(ans)) {
          throw new Error(
            "Using new container is not allowed in toJsonWithReplacer",
          );
        }

        if (typeof ans === "object" && ans != null) {
          return run(ans as any);
        }

        return ans;
      }
    }

    if (typeof value === "object" && value != null) {
      return run(value as Record<string, Value>);
    }

    const ans = replacer(key, value);
    if (isContainer(ans)) {
      throw new Error(
        "Using new container is not allowed in toJsonWithReplacer",
      );
    }

    return ans;
  };

  const run = (layer: Record<string, Value> | Value[]): Value => {
    if (Array.isArray(layer)) {
      return layer
        .map((item, index) => {
          return m(index, item);
        })
        .filter((item): item is NonNullable<typeof item> => item !== undefined);
    }

    const result: Record<string, Value> = {};
    for (const [key, value] of Object.entries(layer)) {
      const ans = m(key, value);
      if (ans !== undefined) {
        result[key] = ans;
      }
    }

    return result;
  };

  const layer = doc.getShallowValue();
  return run(layer);
};

export function idStrToId(idStr: `${number}@${PeerID}`): OpId {
  const [counter, peer] = idStr.split("@");
  return {
    counter: parseInt(counter),
    peer: peer as PeerID,
  };
}

const CONTAINER_READ_CACHE = Symbol("loro.containerReadCache");
const CONTAINER_READ_CACHE_STORE = Symbol("loro.containerReadCacheStore");
const CONTAINER_READ_CACHE_ID = Symbol("loro.containerReadCacheId");
const CONTAINER_CACHED_ID = Symbol("loro.containerCachedId");
const MISSING_MAP_VALUE = Symbol("loro.missingMapValue");
const ENABLE_CONTAINER_OBJECT_CACHE = true;
const ENABLE_SHARED_CONTAINER_READ_CACHE =
  typeof (globalThis as { gc?: unknown }).gc !== "function";
let containerReadCacheEpoch = 0;

type MapReadCache =
  | { type: "keys"; epoch: number; keys: string[] }
  | {
      type: "entries";
      epoch: number;
      keys: string[];
      values: unknown[];
      cursor: number;
      valueByKey?: Map<string, unknown>;
    };

type ListReadCache<T = unknown> =
  | { type: "length"; epoch: number; length: number }
  | { type: "values"; epoch: number; values: T[] };

type TextReadCache = { type: "text"; epoch: number; value: string };

type ContainerReadCache = MapReadCache | ListReadCache | TextReadCache;
type ContainerReadCacheStore = {
  doc: CacheableDoc;
  containers: Record<string, ContainerReadCache | undefined>;
  containerObjects: Map<string, WeakRef<object>>;
};

type CacheableContainer = Container & {
  id: string;
  [CONTAINER_READ_CACHE]?: ContainerReadCache;
  [CONTAINER_READ_CACHE_STORE]?: ContainerReadCacheStore;
  [CONTAINER_READ_CACHE_ID]?: string;
  [CONTAINER_CACHED_ID]?: string;
};

type CacheableDoc = LoroDoc & {
  [CONTAINER_READ_CACHE_STORE]?: ContainerReadCacheStore;
};

function bumpContainerReadCacheEpoch(target?: unknown) {
  containerReadCacheEpoch++;
  const store = (
    target as {
      [CONTAINER_READ_CACHE_STORE]?: ContainerReadCacheStore;
    } | null
  )?.[CONTAINER_READ_CACHE_STORE];
  if (store) {
    store.containers = Object.create(null) as Record<
      string,
      ContainerReadCache | undefined
    >;
    store.containerObjects.clear();
  }
}

function ensureDocReadCacheStore(
  doc: CacheableDoc,
): ContainerReadCacheStore | undefined {
  if (!ENABLE_SHARED_CONTAINER_READ_CACHE) {
    return undefined;
  }

  return (doc[CONTAINER_READ_CACHE_STORE] ??= {
    doc,
    containers: Object.create(null) as Record<
      string,
      ContainerReadCache | undefined
    >,
    containerObjects: new Map(),
  });
}

function getContainerCacheStore(
  container: CacheableContainer,
): ContainerReadCacheStore | undefined {
  return container[CONTAINER_READ_CACHE_STORE];
}

function getContainerCacheId(container: CacheableContainer): string {
  return (container[CONTAINER_READ_CACHE_ID] ??= container.id);
}

function installContainerIdentityCache(prototype: object, kind: ContainerType) {
  const typedPrototype = prototype as CacheableContainer & {
    kind: () => ContainerType;
  };
  (prototype as { [CONTAINER_KIND]?: ContainerType })[CONTAINER_KIND] = kind;
  typedPrototype.kind = function (): ContainerType {
    return kind;
  } as typeof typedPrototype.kind;

  const idDescriptor = Object.getOwnPropertyDescriptor(prototype, "id");
  if (idDescriptor?.get) {
    const originalId = idDescriptor.get;
    Object.defineProperty(prototype, "id", {
      ...idDescriptor,
      get: function (this: CacheableContainer) {
        return (this[CONTAINER_CACHED_ID] ??= originalId.call(this));
      },
    });
  }
}

function cacheContainerObject(
  store: ContainerReadCacheStore | undefined,
  id: string,
  value: unknown,
) {
  if (
    !ENABLE_CONTAINER_OBJECT_CACHE ||
    !store ||
    typeof value !== "object" ||
    value == null
  ) {
    return;
  }

  const existing = store.containerObjects.get(id)?.deref();
  if (existing === value) {
    return;
  }

  const WeakRefCtor = (globalThis as { WeakRef?: typeof WeakRef }).WeakRef;
  if (!WeakRefCtor) {
    return;
  }

  store.containerObjects.set(id, new WeakRefCtor(value));
}

function getCachedContainerObject(
  store: ContainerReadCacheStore | undefined,
  id: string,
): object | undefined {
  if (!ENABLE_CONTAINER_OBJECT_CACHE) {
    return undefined;
  }

  const cached = store?.containerObjects.get(id)?.deref();
  if (cached == null) {
    store?.containerObjects.delete(id);
    return undefined;
  }

  return cached;
}

function getContainerReadCache<T extends ContainerReadCache>(
  container: CacheableContainer,
): T | undefined {
  const store = getContainerCacheStore(container);
  if (store) {
    return store.containers[getContainerCacheId(container)] as T | undefined;
  }

  return container[CONTAINER_READ_CACHE] as T | undefined;
}

function setContainerReadCache(
  container: CacheableContainer,
  cache: ContainerReadCache,
) {
  const store = getContainerCacheStore(container);
  if (store) {
    store.containers[getContainerCacheId(container)] = cache;
  } else {
    container[CONTAINER_READ_CACHE] = cache;
  }
}

function fromCachedReadValue(
  value: unknown,
  store: ContainerReadCacheStore | undefined,
): unknown {
  return tagContainerWithReadCacheStore(value, store);
}

function tagContainerWithReadCacheStore<T>(
  value: T,
  store: ContainerReadCacheStore | undefined,
): T {
  if (!store || !isContainer(value)) {
    return value;
  }

  const container = value as CacheableContainer;
  if (
    container[CONTAINER_READ_CACHE_STORE] === store &&
    !container[CONTAINER_CACHED_ID]
  ) {
    return value;
  }

  container[CONTAINER_READ_CACHE_STORE] = store;
  if (container[CONTAINER_CACHED_ID]) {
    cacheContainerObject(store, container[CONTAINER_CACHED_ID], container);
  }
  return value;
}

function tagContainerChildren<T>(
  values: T[],
  store: ContainerReadCacheStore | undefined,
): T[] {
  if (!store) {
    return values;
  }

  for (const value of values) {
    tagContainerWithReadCacheStore(value, store);
  }
  return values;
}

function tagMapEntries(
  entries: [string, unknown][],
  store: ContainerReadCacheStore | undefined,
): [string, unknown][] {
  if (!store) {
    return entries;
  }

  for (const entry of entries) {
    tagContainerWithReadCacheStore(entry[1], store);
  }
  return entries;
}

function wrapCacheInvalidatingMethods(
  prototype: object,
  methods: PropertyKey[],
) {
  for (const method of methods) {
    const descriptor = Object.getOwnPropertyDescriptor(prototype, method);
    if (!descriptor || typeof descriptor.value !== "function") {
      continue;
    }

    const original = descriptor.value as (...args: unknown[]) => unknown;
    Object.defineProperty(prototype, method, {
      ...descriptor,
      value: function (this: unknown, ...args: unknown[]) {
        bumpContainerReadCacheEpoch(this);
        return tagContainerWithReadCacheStore(
          original.apply(this, args),
          (
            this as {
              [CONTAINER_READ_CACHE_STORE]?: ContainerReadCacheStore;
            } | null
          )?.[CONTAINER_READ_CACHE_STORE],
        );
      },
    });
  }
}

function wrapContainerReturningMethods(
  prototype: object,
  methods: PropertyKey[],
) {
  for (const method of methods) {
    const descriptor = Object.getOwnPropertyDescriptor(prototype, method);
    if (!descriptor || typeof descriptor.value !== "function") {
      continue;
    }

    const original = descriptor.value as (...args: unknown[]) => unknown;
    Object.defineProperty(prototype, method, {
      ...descriptor,
      value: function (this: CacheableDoc, ...args: unknown[]) {
        const store = ensureDocReadCacheStore(this);
        if (method === "getContainerById" && typeof args[0] === "string") {
          const cached = getCachedContainerObject(store, args[0]);
          if (cached != null) {
            return tagContainerWithReadCacheStore(cached, store);
          }
        }

        const value = original.apply(this, args);
        if (method === "getContainerById" && typeof args[0] === "string") {
          cacheContainerObject(store, args[0], value);
        }

        return tagContainerWithReadCacheStore(value, store);
      },
    });
  }
}

function installMapReadCache() {
  const prototype = LoroMap.prototype as unknown as CacheableContainer & {
    keys: () => string[];
    entries: () => [string, unknown][];
    values: () => unknown[];
    get: (key: string) => unknown;
    __entriesFlat?: () => unknown[];
  };
  const originalKeys = prototype.keys;
  const originalEntries = prototype.entries;
  const originalEntriesFlat = prototype.__entriesFlat;
  const originalValues = prototype.values;
  const originalGet = prototype.get;

  const readFlatEntries = function (container: CacheableContainer) {
    const store = getContainerCacheStore(container);
    const keys: string[] = [];
    const values: unknown[] = [];

    if (typeof originalEntriesFlat === "function") {
      const flatEntries = originalEntriesFlat.call(container);
      for (let index = 0; index + 1 < flatEntries.length; index += 2) {
        const entryKey = flatEntries[index];
        if (typeof entryKey !== "string") {
          continue;
        }

        const value = tagContainerWithReadCacheStore(
          flatEntries[index + 1],
          store,
        );
        keys.push(entryKey);
        values.push(value);
      }
    } else {
      for (const [entryKey, value] of tagMapEntries(
        originalEntries.call(container),
        store,
      )) {
        keys.push(entryKey);
        values.push(value);
      }
    }

    const cache: MapReadCache = {
      type: "entries",
      epoch: containerReadCacheEpoch,
      keys,
      values,
      cursor: 0,
    };
    setContainerReadCache(container, cache);
    return cache;
  };

  const getCachedMapValue = function (
    cache: Extract<MapReadCache, { type: "entries" }>,
    key: string,
  ): unknown {
    if (cache.keys[cache.cursor] === key) {
      return cache.values[cache.cursor++];
    }

    let valueByKey = cache.valueByKey;
    if (!valueByKey) {
      valueByKey = new Map();
      for (let index = 0; index < cache.keys.length; index++) {
        valueByKey.set(cache.keys[index], cache.values[index]);
      }
      cache.valueByKey = valueByKey;
    }

    return valueByKey.has(key) ? valueByKey.get(key) : MISSING_MAP_VALUE;
  };

  prototype.keys = function () {
    const cache = getContainerReadCache<MapReadCache>(this);
    if (cache?.epoch === containerReadCacheEpoch) {
      if (cache.type === "entries") {
        cache.cursor = 0;
        return cache.keys.slice();
      }

      if (cache.type === "keys") {
        return cache.keys.slice();
      }
    }

    if (getContainerCacheStore(this)) {
      return readFlatEntries(this).keys.slice();
    }

    const keys = originalKeys.call(this);
    setContainerReadCache(this, {
      type: "keys",
      epoch: containerReadCacheEpoch,
      keys: keys.slice(),
    });
    return keys;
  };

  prototype.get = function (key: string) {
    const cache = getContainerReadCache<MapReadCache>(this);
    if (cache?.type === "entries" && cache.epoch === containerReadCacheEpoch) {
      const value = getCachedMapValue(cache, key);
      return value === MISSING_MAP_VALUE
        ? undefined
        : fromCachedReadValue(value, getContainerCacheStore(this));
    }

    if (cache?.type === "keys" && cache.epoch === containerReadCacheEpoch) {
      const entries = readFlatEntries(this);
      const store = getContainerCacheStore(this);
      const value = getCachedMapValue(entries, key);
      return value === MISSING_MAP_VALUE
        ? undefined
        : fromCachedReadValue(value, store);
    }

    return originalGet.call(this, key);
  };

  prototype.entries = function () {
    const cache = getContainerReadCache<MapReadCache>(this);
    const store = getContainerCacheStore(this);
    if (cache?.type === "entries" && cache.epoch === containerReadCacheEpoch) {
      return cache.keys.map(
        (key, index) =>
          [key, fromCachedReadValue(cache.values[index], store)] as [
            string,
            unknown,
          ],
      );
    }

    const entries = readFlatEntries(this);
    return entries.keys.map(
      (key, index) =>
        [key, fromCachedReadValue(entries.values[index], store)] as [
          string,
          unknown,
        ],
    );
  };

  prototype.values = function () {
    const cache = getContainerReadCache<MapReadCache>(this);
    const store = getContainerCacheStore(this);
    if (cache?.type === "entries" && cache.epoch === containerReadCacheEpoch) {
      return cache.values.map((value) => fromCachedReadValue(value, store));
    }

    const values = originalValues.call(this);
    tagContainerChildren(values, store);
    return values;
  };

  wrapCacheInvalidatingMethods(prototype, [
    "set",
    "delete",
    "getOrCreateContainer",
    "setContainer",
    "clear",
  ]);
}

function installListReadCache(
  prototype: object,
  mutatingMethods: PropertyKey[],
) {
  const typedPrototype = prototype as CacheableContainer & {
    toArray: () => unknown[];
    get: (index: number) => unknown;
  };
  const lengthDescriptor = Object.getOwnPropertyDescriptor(prototype, "length");
  const originalToArray = typedPrototype.toArray;
  const originalGet = typedPrototype.get;

  const readArrayValues = function (container: CacheableContainer) {
    const store = getContainerCacheStore(container);
    const values = tagContainerChildren(
      originalToArray.call(container as typeof typedPrototype),
      store,
    );
    const cache: ListReadCache = {
      type: "values",
      epoch: containerReadCacheEpoch,
      values,
    };
    setContainerReadCache(container, cache);
    return { values, cache };
  };

  if (lengthDescriptor?.get) {
    const originalLength = lengthDescriptor.get;
    Object.defineProperty(prototype, "length", {
      ...lengthDescriptor,
      get: function (this: typeof typedPrototype) {
        const cache = getContainerReadCache<ListReadCache>(this);
        if (cache?.epoch === containerReadCacheEpoch) {
          return cache.type === "values" ? cache.values.length : cache.length;
        }

        if (getContainerCacheStore(this)) {
          return readArrayValues(this).cache.values.length;
        }

        const length = originalLength.call(this);
        setContainerReadCache(this, {
          type: "length",
          epoch: containerReadCacheEpoch,
          length,
        });
        return length;
      },
    });
  }

  typedPrototype.get = function (index: number) {
    const cache = getContainerReadCache<ListReadCache>(this);
    const store = getContainerCacheStore(this);
    if (cache?.type === "values" && cache.epoch === containerReadCacheEpoch) {
      return index < cache.values.length
        ? fromCachedReadValue(cache.values[index], store)
        : undefined;
    }

    if (cache?.type === "length" && cache.epoch === containerReadCacheEpoch) {
      const { values } = readArrayValues(this);
      return index < values.length ? values[index] : undefined;
    }

    return originalGet.call(this, index);
  };

  typedPrototype.toArray = function () {
    const cache = getContainerReadCache<ListReadCache>(this);
    const store = getContainerCacheStore(this);
    if (cache?.type === "values" && cache.epoch === containerReadCacheEpoch) {
      return cache.values.map((value) => fromCachedReadValue(value, store));
    }

    const values = tagContainerChildren(originalToArray.call(this), store);
    setContainerReadCache(this, {
      type: "values",
      epoch: containerReadCacheEpoch,
      values,
    });
    return values.slice();
  };

  wrapCacheInvalidatingMethods(prototype, mutatingMethods);
}

function installTextReadCache() {
  const prototype = LoroText.prototype as unknown as CacheableContainer & {
    toString: () => string;
    toJSON: () => string;
    getShallowValue: () => string;
  };
  const originalToString = prototype.toString;

  const readText = function (this: CacheableContainer) {
    const cache = getContainerReadCache<TextReadCache>(this);
    if (cache?.type === "text" && cache.epoch === containerReadCacheEpoch) {
      return cache.value;
    }

    const value = originalToString.call(this);
    setContainerReadCache(this, {
      type: "text",
      epoch: containerReadCacheEpoch,
      value,
    });
    return value;
  };

  prototype.toString = readText;
  prototype.toJSON = readText;
  prototype.getShallowValue = readText;

  wrapCacheInvalidatingMethods(prototype, [
    "update",
    "updateByLine",
    "insert",
    "insertUtf8",
    "delete",
    "deleteUtf8",
    "splice",
    "push",
    "mark",
    "unmark",
  ]);
}

wrapContainerReturningMethods(LoroDoc.prototype, [
  "getMap",
  "getList",
  "getMovableList",
  "getText",
  "getTree",
  "getCounter",
  "getByPath",
  "getContainerById",
]);
installContainerIdentityCache(LoroMap.prototype, "Map");
installContainerIdentityCache(LoroList.prototype, "List");
installContainerIdentityCache(LoroMovableList.prototype, "MovableList");
installContainerIdentityCache(LoroText.prototype, "Text");
installContainerIdentityCache(LoroTree.prototype, "Tree");
installContainerIdentityCache(LoroCounter.prototype, "Counter");
installMapReadCache();
installTextReadCache();
installListReadCache(LoroList.prototype, [
  "insert",
  "delete",
  "insertContainer",
  "pushContainer",
  "push",
  "pop",
  "clear",
]);
installListReadCache(LoroMovableList.prototype, [
  "insert",
  "delete",
  "insertContainer",
  "pushContainer",
  "move",
  "set",
  "setContainer",
  "push",
  "pop",
  "clear",
]);
wrapCacheInvalidatingMethods(LoroDoc.prototype, [
  "setDetachedEditing",
  "attach",
  "detach",
  "fork",
  "forkAt",
  "checkoutToLatest",
  "checkout",
  "commit",
  "revertTo",
  "export",
  "exportJsonUpdates",
  "exportJsonInIdSpan",
  "importJsonUpdates",
  "import",
  "importUpdateBatch",
  "importBatch",
  "travelChangeAncestors",
  "applyDiff",
  "setPeerId",
]);
wrapCacheInvalidatingMethods(UndoManager.prototype, ["undo", "redo"]);

const CALL_PENDING_EVENTS_WRAPPED = Symbol("loro.callPendingEventsWrapped");

function decorateMethod(prototype: object, method: PropertyKey) {
  const descriptor = Object.getOwnPropertyDescriptor(prototype, method);
  if (!descriptor || typeof descriptor.value !== "function") {
    return;
  }

  const original = descriptor.value as (...args: unknown[]) => unknown;
  if ((original as any)[CALL_PENDING_EVENTS_WRAPPED]) {
    return;
  }

  const wrapped = function (this: unknown, ...args: unknown[]) {
    let result;
    try {
      result = original.apply(this, args);
      return result;
    } finally {
      if (result && typeof (result as Promise<unknown>).then === "function") {
        (result as Promise<unknown>).finally(() => {
          callPendingEvents();
        });
      } else {
        callPendingEvents();
      }
    }
  };

  (wrapped as any)[CALL_PENDING_EVENTS_WRAPPED] = true;

  Object.defineProperty(prototype, method, {
    ...descriptor,
    value: wrapped,
  });
}

function decorateMethods(prototype: object, methods: PropertyKey[]) {
  for (const method of methods) {
    decorateMethod(prototype, method);
  }
}

function decorateAllPrototypeMethods(prototype: object) {
  const visited = new Set<PropertyKey>();
  let current: object | null = prototype;
  while (
    current &&
    current !== Object.prototype &&
    current !== Function.prototype
  ) {
    for (const property of Object.getOwnPropertyNames(current)) {
      if (property === "constructor" || visited.has(property)) {
        continue;
      }
      visited.add(property);
      decorateMethod(current, property);
    }

    for (const symbol of Object.getOwnPropertySymbols(current)) {
      if (visited.has(symbol)) {
        continue;
      }
      visited.add(symbol);
      decorateMethod(current, symbol);
    }

    current = Object.getPrototypeOf(current) as object | null;
  }
}

decorateMethods(LoroDoc.prototype, [
  "setDetachedEditing",
  "attach",
  "detach",
  "fork",
  "forkAt",
  "checkoutToLatest",
  "checkout",
  "commit",
  "getCursorPos",
  "revertTo",
  "export",
  "exportJsonUpdates",
  "exportJsonInIdSpan",
  "importJsonUpdates",
  "import",
  "importUpdateBatch",
  "importBatch",
  "travelChangeAncestors",
  "getChangedContainersIn",
  "diff",
  "applyDiff",
  "setPeerId",
]);

decorateMethods(EphemeralStoreWasm.prototype, [
  "set",
  "delete",
  "apply",
  "removeOutdated",
]);

decorateMethods(UndoManager.prototype, ["undo", "redo"]);
