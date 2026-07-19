import { PostcardReader, PostcardWriter } from "../codec/postcard";
import { readPostcardValue, writePostcardValue } from "../codec/value";
import { formatContainerId, isContainerId, parseContainerId, parsePeerId } from "./ids";
/** The low-level awareness class exported by `loro-crdt`. */
export class AwarenessWasm {
    #peer;
    #timeout;
    #states = new Map();
    constructor(peer, timeout) {
        this.#peer = parsePeerId(peer);
        this.#timeout = assertTimeout(timeout);
    }
    free() { }
    encode(peers) {
        const now = Date.now();
        const states = [];
        for (const peerInput of peers) {
            const peer = parsePeerId(peerInput);
            const state = this.#states.get(peer);
            if (state === undefined || now - state.timestamp > this.#timeout)
                continue;
            states.push({ peer, counter: state.counter, value: state.value });
        }
        return encodeAwarenessStates(states);
    }
    encodeAll() {
        return this.encode(this.peers());
    }
    apply(bytes) {
        let decoded;
        try {
            decoded = decodeAwarenessStates(bytes);
        }
        catch (error) {
            throw new Error(`Failed to decode awareness data: ${errorMessage(error)}`);
        }
        const now = Date.now();
        const updated = [];
        const added = [];
        for (const state of decoded) {
            const current = this.#states.get(state.peer);
            if (state.peer === this.#peer ||
                (current !== undefined && current.counter >= state.counter)) {
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
    setLocalState(value) {
        const current = this.#states.get(this.#peer);
        this.#states.set(this.#peer, {
            value: valueToEncoded(value),
            counter: (current?.counter ?? 0) + 1,
            timestamp: Date.now(),
        });
    }
    peer() {
        return peerString(this.#peer);
    }
    getAllStates() {
        const output = {};
        for (const [peer, state] of this.#states) {
            output[peerString(peer)] = encodedToValue(state.value);
        }
        return output;
    }
    getState(peer) {
        const state = this.#states.get(parsePeerId(peer));
        return state === undefined ? undefined : encodedToValue(state.value);
    }
    getTimestamp(peer) {
        return this.#states.get(parsePeerId(peer))?.timestamp;
    }
    removeOutdated() {
        const now = Date.now();
        const removed = [];
        for (const peer of this.orderedPeers()) {
            const state = this.#states.get(peer);
            if (now - state.timestamp <= this.#timeout)
                continue;
            this.#states.delete(peer);
            removed.push(peerString(peer));
        }
        return removed;
    }
    length() {
        return this.#states.size;
    }
    isEmpty() {
        return this.#states.size === 0;
    }
    peers() {
        return this.orderedPeers().map(peerString);
    }
    orderedPeers() {
        const peers = [];
        if (this.#states.has(this.#peer))
            peers.push(this.#peer);
        for (const peer of this.#states.keys()) {
            if (peer !== this.#peer)
                peers.push(peer);
        }
        return peers;
    }
}
/** @deprecated Use `EphemeralStore` for new code. */
export class Awareness {
    inner;
    #peer;
    #timeout;
    #listeners = new Set();
    #timer;
    constructor(peer, timeout = 30000) {
        this.inner = new AwarenessWasm(peer, timeout);
        this.#peer = this.inner.peer();
        this.#timeout = timeout;
    }
    apply(bytes, origin = "remote") {
        const { updated, added } = this.inner.apply(bytes);
        for (const listener of this.#listeners) {
            listener({ updated, added, removed: [] }, origin);
        }
        this.startTimerIfNotEmpty();
    }
    setLocalState(state) {
        const wasEmpty = this.inner.getState(this.#peer) === undefined;
        this.inner.setLocalState(state);
        const peer = this.inner.peer();
        for (const listener of this.#listeners) {
            listener(wasEmpty
                ? { updated: [], added: [peer], removed: [] }
                : { updated: [peer], added: [], removed: [] }, "local");
        }
        this.startTimerIfNotEmpty();
    }
    getLocalState() {
        return this.inner.getState(this.#peer);
    }
    getAllStates() {
        return this.inner.getAllStates();
    }
    encode(peers) {
        return this.inner.encode(peers);
    }
    encodeAll() {
        return this.inner.encodeAll();
    }
    addListener(listener) {
        this.#listeners.add(listener);
    }
    removeListener(listener) {
        this.#listeners.delete(listener);
    }
    peers() {
        return this.inner.peers();
    }
    destroy() {
        if (this.#timer !== undefined)
            clearInterval(this.#timer);
        this.#timer = undefined;
        this.#listeners.clear();
    }
    startTimerIfNotEmpty() {
        if (this.inner.isEmpty() || this.#timer !== undefined)
            return;
        this.#timer = setInterval(() => {
            const removed = this.inner.removeOutdated();
            if (removed.length > 0) {
                for (const listener of this.#listeners) {
                    listener({ updated: [], added: [], removed }, "timeout");
                }
            }
            if (!this.inner.isEmpty())
                return;
            clearInterval(this.#timer);
            this.#timer = undefined;
        }, this.#timeout / 2);
    }
}
/** The low-level ephemeral key-value store exported by `loro-crdt`. */
export class EphemeralStoreWasm {
    #timeout;
    #states = new Map();
    #listeners = new Set();
    #localListeners = new Set();
    #pendingEvents = [];
    #pendingLocalUpdates = [];
    // Adds made from inside a dispatch are queued and flushed after that
    // event's dispatch, preserving the former per-event snapshot semantics
    // without copying the listener set per emission. Removals apply to the
    // live set immediately; Set iteration skips not-yet-visited removals,
    // which matches the old `has` guard.
    #queuedListeners = [];
    #queuedLocalListeners = [];
    #deferringListeners = false;
    #deferringLocalListeners = false;
    #emittingEvents = false;
    #emittingLocalUpdates = false;
    constructor(timeout) {
        this.#timeout = assertTimeout(timeout);
    }
    free() { }
    set(key, value) {
        this.setLocal(key, valueToEncoded(value));
    }
    delete(key) {
        this.setLocal(key, undefined);
    }
    get(key) {
        const value = this.#states.get(key)?.value;
        return value === undefined ? undefined : encodedToValue(value);
    }
    getAllStates() {
        const output = {};
        for (const [key, state] of this.#states) {
            if (state.value !== undefined)
                output[key] = encodedToValue(state.value);
        }
        return output;
    }
    subscribeLocalUpdates(listener) {
        if (this.#deferringLocalListeners)
            this.#queuedLocalListeners.push(listener);
        else
            this.#localListeners.add(listener);
        return () => {
            this.#localListeners.delete(listener);
            const queued = this.#queuedLocalListeners.indexOf(listener);
            if (queued !== -1)
                this.#queuedLocalListeners.splice(queued, 1);
        };
    }
    subscribe(listener) {
        if (this.#deferringListeners)
            this.#queuedListeners.push(listener);
        else
            this.#listeners.add(listener);
        return () => {
            this.#listeners.delete(listener);
            const queued = this.#queuedListeners.indexOf(listener);
            if (queued !== -1)
                this.#queuedListeners.splice(queued, 1);
        };
    }
    encode(key) {
        const state = this.#states.get(key);
        if (state === undefined)
            return encodeEphemeralStates([]);
        if (Date.now() - state.timestamp > this.#timeout)
            return new Uint8Array();
        return encodeEphemeralStates([{ key, ...state }]);
    }
    encodeAll() {
        const now = Date.now();
        const states = [];
        for (const [key, state] of this.#states) {
            if (now - state.timestamp <= this.#timeout)
                states.push({ key, ...state });
        }
        return encodeEphemeralStates(states);
    }
    apply(bytes) {
        let decoded;
        try {
            decoded = decodeEphemeralStates(bytes);
        }
        catch (error) {
            throw new Error(`Failed to decode data: ${errorMessage(error)}`);
        }
        const added = [];
        const updated = [];
        const removed = [];
        const now = Date.now();
        for (const state of decoded) {
            if (now - state.timestamp > this.#timeout)
                continue;
            const old = this.#states.get(state.key);
            if (old !== undefined && old.timestamp >= state.timestamp)
                continue;
            this.#states.set(state.key, { value: state.value, timestamp: state.timestamp });
            if (old !== undefined && state.value !== undefined)
                updated.push(state.key);
            else if (old === undefined && state.value !== undefined)
                added.push(state.key);
            else if (old !== undefined && state.value === undefined)
                removed.push(state.key);
        }
        this.emit({ by: "import", added, updated, removed });
    }
    removeOutdated() {
        const now = Date.now();
        const removed = [];
        for (const [key, state] of this.#states) {
            if (now - state.timestamp <= this.#timeout)
                continue;
            this.#states.delete(key);
            if (state.value !== undefined)
                removed.push(key);
        }
        this.emit({ by: "timeout", added: [], updated: [], removed });
    }
    isEmpty() {
        return this.#states.size === 0;
    }
    keys() {
        const output = [];
        for (const [key, state] of this.#states) {
            if (state.value !== undefined)
                output.push(key);
        }
        return output;
    }
    setLocal(key, value) {
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
    emit(event) {
        if (this.#emittingEvents) {
            this.#pendingEvents.push(event);
            return;
        }
        this.#emittingEvents = true;
        const pending = [event];
        try {
            while (pending.length > 0) {
                const current = pending.pop();
                this.#deferringListeners = true;
                try {
                    for (const listener of this.#listeners)
                        listener(current);
                }
                finally {
                    this.#deferringListeners = false;
                    for (const listener of this.#queuedListeners)
                        this.#listeners.add(listener);
                    this.#queuedListeners.length = 0;
                }
                pending.push(...this.#pendingEvents.splice(0));
            }
        }
        finally {
            this.#emittingEvents = false;
            this.#pendingEvents.length = 0;
        }
    }
    emitLocalUpdate(bytes) {
        if (this.#emittingLocalUpdates) {
            this.#pendingLocalUpdates.push(bytes);
            return;
        }
        this.#emittingLocalUpdates = true;
        const pending = [bytes];
        try {
            while (pending.length > 0) {
                const current = pending.pop();
                this.#deferringLocalListeners = true;
                try {
                    for (const listener of this.#localListeners)
                        listener(current);
                }
                finally {
                    this.#deferringLocalListeners = false;
                    for (const listener of this.#queuedLocalListeners) {
                        this.#localListeners.add(listener);
                    }
                    this.#queuedLocalListeners.length = 0;
                }
                pending.push(...this.#pendingLocalUpdates.splice(0));
            }
        }
        finally {
            this.#emittingLocalUpdates = false;
            this.#pendingLocalUpdates.length = 0;
        }
    }
}
/** A typed, automatically expiring wrapper around `EphemeralStoreWasm`. */
export class EphemeralStore {
    inner;
    #timeout;
    #timer;
    constructor(timeout = 30000) {
        this.inner = new EphemeralStoreWasm(timeout);
        this.#timeout = timeout;
    }
    apply(bytes) {
        this.inner.apply(bytes);
        this.startTimerIfNotEmpty();
    }
    set(key, value) {
        this.inner.set(key, value);
        this.startTimerIfNotEmpty();
    }
    delete(key) {
        this.inner.delete(key);
    }
    get(key) {
        return this.inner.get(key);
    }
    getAllStates() {
        return this.inner.getAllStates();
    }
    encode(key) {
        return this.inner.encode(key);
    }
    encodeAll() {
        return this.inner.encodeAll();
    }
    keys() {
        return this.inner.keys();
    }
    destroy() {
        if (this.#timer !== undefined)
            clearInterval(this.#timer);
        this.#timer = undefined;
    }
    subscribe(listener) {
        return this.inner.subscribe(listener);
    }
    subscribeLocalUpdates(listener) {
        return this.inner.subscribeLocalUpdates(listener);
    }
    startTimerIfNotEmpty() {
        if (this.inner.isEmpty() || this.#timer !== undefined)
            return;
        this.#timer = setInterval(() => {
            this.inner.removeOutdated();
            if (!this.inner.isEmpty())
                return;
            clearInterval(this.#timer);
            this.#timer = undefined;
        }, this.#timeout / 2);
    }
}
function encodeAwarenessStates(states) {
    const writer = new PostcardWriter();
    writer.writeUsize(states.length);
    for (const state of states) {
        writer.writeU64(state.peer);
        writer.writeI32(state.counter);
        writePostcardValue(writer, state.value);
    }
    return writer.toUint8Array();
}
function decodeAwarenessStates(bytes) {
    const reader = new PostcardReader(bytes);
    const states = reader.readArray((input) => ({
        peer: input.readU64(),
        counter: input.readI32(),
        value: readPostcardValue(input),
    }));
    reader.assertEnd();
    return states;
}
function encodeEphemeralStates(states) {
    const writer = new PostcardWriter();
    writer.writeUsize(states.length);
    for (const state of states) {
        writer.writeString(state.key);
        writer.writeOption(state.value, writePostcardValue);
        writer.writeI64(BigInt(state.timestamp));
    }
    return writer.toUint8Array();
}
function decodeEphemeralStates(bytes) {
    const reader = new PostcardReader(bytes);
    const states = reader.readArray((input) => ({
        key: input.readString(),
        value: input.readOption(readPostcardValue),
        timestamp: Number(input.readI64()),
    }));
    reader.assertEnd();
    return states;
}
function valueToEncoded(value, depth = 0) {
    if (depth > 512)
        throw new RangeError("LoroValue nesting depth exceeded");
    if (value === null || value === undefined)
        return { type: "null" };
    if (typeof value === "boolean")
        return { type: "bool", value };
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
    if (value instanceof Uint8Array)
        return { type: "binary", value: value.slice() };
    if (Array.isArray(value)) {
        return { type: "list", value: value.map((item) => valueToEncoded(item, depth + 1)) };
    }
    if (value instanceof Map) {
        return {
            type: "map",
            value: [...value].map(([key, item]) => {
                if (typeof key !== "string")
                    throw new TypeError("Map keys must be strings");
                return [key, valueToEncoded(item, depth + 1)];
            }),
        };
    }
    if (typeof value === "object") {
        return {
            type: "map",
            value: Object.entries(value).map(([key, item]) => [key, valueToEncoded(item, depth + 1)]),
        };
    }
    throw new TypeError(`unsupported LoroValue type: ${typeof value}`);
}
function encodedToValue(value) {
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
            return Object.fromEntries(value.value.map(([key, item]) => [key, encodedToValue(item)]));
        case "container":
            return formatContainerId(value.value);
    }
}
function assertTimeout(timeout) {
    if (!Number.isFinite(timeout))
        throw new TypeError("timeout must be finite");
    return timeout;
}
function peerString(peer) {
    return peer.toString();
}
function errorMessage(error) {
    return error instanceof Error ? error.message : String(error);
}
