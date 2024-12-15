export * from "loro-wasm";
export type * from "loro-wasm";
import {
    AwarenessWasm,
    PeerID,
    Container,
    ContainerID,
    ContainerType,
    LoroCounter,
    LoroDoc,
    LoroList,
    LoroMap,
    LoroText,
    LoroTree,
    OpId,
    Value,
    AwarenessListener,
} from "loro-wasm";

/**
 * @deprecated Please use LoroDoc
 */
export class Loro extends LoroDoc { }

const CONTAINER_TYPES = [
    "Map",
    "Text",
    "List",
    "Tree",
    "MovableList",
    "Counter",
];

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

    const p = Object.getPrototypeOf(value);
    if (p == null || typeof p !== "object" || typeof p["kind"] !== "function") {
        return false;
    }

    return CONTAINER_TYPES.includes(value.kind());
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
): T extends LoroText ? "Text"
    : T extends LoroMap<any> ? "Map"
    : T extends LoroTree<any> ? "Tree"
    : T extends LoroList<any> ? "List"
    : T extends LoroCounter ? "Counter"
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
 * Awareness is a structure that allows to track the ephemeral state of the peers.
 *
 * If we don't receive a state update from a peer within the timeout, we will remove their state.
 * The timeout is in milliseconds. This can be used to handle the off-line state of a peer.
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

LoroDoc.prototype.toJsonWithReplacer = function (replacer: (key: string | number, value: Value | Container) => Value | Container | undefined) {
    const doc = this;
    const m = (key: string | number, value: Value): Value | undefined => {
        if (typeof value === "string") {
            if (isContainerId(value)) {
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
                    throw new Error("Using new container is not allowed in toJsonWithReplacer");
                }

                return ans;
            }
        }

        const ans = replacer(key, value);
        if (isContainer(ans)) {
            throw new Error("Using new container is not allowed in toJsonWithReplacer");
        }

        return ans;
    }

    const run = (layer: Record<string, Value> | Value[]): Value => {
        if (Array.isArray(layer)) {
            return layer.map((item, index) => {
                return m(index, item);
            }).filter((item): item is NonNullable<typeof item> => item !== undefined);
        }

        const result: Record<string, Value> = {};
        for (const [key, value] of Object.entries(layer)) {
            const ans = m(key, value);
            if (ans !== undefined) {
                result[key] = ans;
            }
        }

        return result;
    }

    const layer = this.getShallowValue();
    return run(layer);
}

