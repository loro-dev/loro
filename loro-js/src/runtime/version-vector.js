import { decodePostcardVersionVector, encodePostcardVersionVector, } from "../codec/version";
import { formatOpId, parseOpId, parsePeerId, peerIdToString } from "./ids";
export class VersionVector {
    #values = new Map();
    constructor(input) {
        if (input == null)
            return;
        if (input instanceof Uint8Array) {
            for (const id of decodePostcardVersionVector(input))
                this.#values.set(id.peer, id.counter);
            return;
        }
        if (input instanceof VersionVector) {
            for (const [peer, counter] of input.#values)
                this.#values.set(peer, counter);
            return;
        }
        const entries = input instanceof Map ? input.entries() : Object.entries(input);
        for (const [peer, counter] of entries)
            this.set(peer, counter);
    }
    free() { }
    static parseJSON(input) {
        return new VersionVector(input);
    }
    static decode(bytes) {
        return new VersionVector(bytes);
    }
    encode() {
        return encodePostcardVersionVector(this.codecEntries());
    }
    toJSON() {
        return new Map([...this.#values].map(([peer, counter]) => [peerIdToString(peer), counter]));
    }
    get(peer) {
        return this.#values.get(parsePeerId(peer));
    }
    compare(other) {
        let less = false;
        let greater = false;
        for (const [peer, left] of this.#values) {
            const right = other.#values.get(peer) ?? 0;
            if (left < right)
                less = true;
            else if (left > right)
                greater = true;
            if (less && greater)
                return undefined;
        }
        // Peers present only in the other vector compare as 0 on this side.
        for (const [peer, right] of other.#values) {
            if (this.#values.has(peer))
                continue;
            if (right > 0)
                less = true;
            else if (right < 0)
                greater = true;
            if (less && greater)
                return undefined;
        }
        return less ? -1 : greater ? 1 : 0;
    }
    setEnd(id) {
        const parsed = parseOpId(id);
        this.set(parsed.peer, parsed.counter);
    }
    setLast(id) {
        const parsed = parseOpId(id);
        this.set(parsed.peer, parsed.counter + 1);
    }
    remove(peer) {
        this.#values.delete(parsePeerId(peer));
    }
    length() {
        return this.#values.size;
    }
    clone() {
        return new VersionVector(this);
    }
    codecEntries() {
        const entries = this._codecEntriesUnsorted();
        entries.sort((left, right) => left.peer < right.peer ? -1 : left.peer > right.peer ? 1 : 0);
        return entries;
    }
    _codecEntriesUnsorted() {
        const entries = [];
        for (const [peer, counter] of this.#values) {
            if (counter > 0)
                entries.push({ peer, counter });
        }
        return entries;
    }
    publicEntries() {
        return this.codecEntries().map(formatOpId);
    }
    set(peer, counter) {
        if (!Number.isSafeInteger(counter) || counter < 0 || counter > 2147483647) {
            throw new RangeError(`version counter is out of range: ${counter}`);
        }
        const parsed = parsePeerId(peer);
        if (counter === 0)
            this.#values.delete(parsed);
        else
            this.#values.set(parsed, counter);
    }
    merge(other) {
        for (const [peer, counter] of other.#values) {
            if (counter > (this.#values.get(peer) ?? 0))
                this.#values.set(peer, counter);
        }
    }
}
