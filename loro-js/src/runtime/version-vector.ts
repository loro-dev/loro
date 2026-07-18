import {
  decodePostcardVersionVector,
  encodePostcardVersionVector,
} from "../codec/version";
import { formatOpId, parseOpId, parsePeerId, peerIdToString } from "./ids";
import type { OpId, PeerID, PeerIdInput } from "./types";

export type VersionVectorInput =
  | VersionVector
  | Map<string, number>
  | Readonly<Record<string, number>>
  | Uint8Array
  | null
  | undefined;

export class VersionVector {
  readonly #values = new Map<bigint, number>();

  constructor(input?: VersionVectorInput) {
    if (input == null) return;
    if (input instanceof Uint8Array) {
      for (const id of decodePostcardVersionVector(input))
        this.#values.set(id.peer, id.counter);
      return;
    }
    if (input instanceof VersionVector) {
      for (const [peer, counter] of input.#values) this.#values.set(peer, counter);
      return;
    }
    const entries = input instanceof Map ? input.entries() : Object.entries(input);
    for (const [peer, counter] of entries) this.set(peer, counter);
  }

  free(): void {}

  static parseJSON(input: Map<string, number>): VersionVector {
    return new VersionVector(input);
  }

  static decode(bytes: Uint8Array): VersionVector {
    return new VersionVector(bytes);
  }

  encode(): Uint8Array {
    return encodePostcardVersionVector(this.codecEntries());
  }

  toJSON(): Map<PeerID, number> {
    return new Map(
      [...this.#values].map(([peer, counter]) => [peerIdToString(peer), counter]),
    );
  }

  get(peer: PeerIdInput): number | undefined {
    return this.#values.get(parsePeerId(peer));
  }

  compare(other: VersionVector): -1 | 0 | 1 | undefined {
    let less = false;
    let greater = false;
    for (const peer of new Set([...this.#values.keys(), ...other.#values.keys()])) {
      const left = this.#values.get(peer) ?? 0;
      const right = other.#values.get(peer) ?? 0;
      less ||= left < right;
      greater ||= left > right;
    }
    if (less && greater) return undefined;
    return less ? -1 : greater ? 1 : 0;
  }

  setEnd(id: OpId): void {
    const parsed = parseOpId(id);
    this.set(parsed.peer, parsed.counter);
  }

  setLast(id: OpId): void {
    const parsed = parseOpId(id);
    this.set(parsed.peer, parsed.counter + 1);
  }

  remove(peer: PeerIdInput): void {
    this.#values.delete(parsePeerId(peer));
  }

  length(): number {
    return this.#values.size;
  }

  clone(): VersionVector {
    return new VersionVector(this);
  }

  codecEntries(): { peer: bigint; counter: number }[] {
    return this._codecEntriesUnsorted()
      .map(({ peer, counter }) => [peer, counter] as const)
      .sort(([left], [right]) => (left < right ? -1 : left > right ? 1 : 0))
      .map(([peer, counter]) => ({ peer, counter }));
  }

  _codecEntriesUnsorted(): { peer: bigint; counter: number }[] {
    return [...this.#values].flatMap(([peer, counter]) =>
      counter > 0 ? [{ peer, counter }] : [],
    );
  }

  publicEntries(): OpId[] {
    return this.codecEntries().map(formatOpId);
  }

  set(peer: PeerIdInput | bigint, counter: number): void {
    if (!Number.isSafeInteger(counter) || counter < 0 || counter > 0x7fff_ffff) {
      throw new RangeError(`version counter is out of range: ${counter}`);
    }
    const parsed = parsePeerId(peer);
    if (counter === 0) this.#values.delete(parsed);
    else this.#values.set(parsed, counter);
  }

  merge(other: VersionVector): void {
    for (const [peer, counter] of other.#values) {
      if (counter > (this.#values.get(peer) ?? 0)) this.#values.set(peer, counter);
    }
  }
}
