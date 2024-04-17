import { AwarenessWasm, PeerID, Value } from "loro-wasm";

export type AwarenessListener = (
  arg: { updated: PeerID[]; added: PeerID[]; removed: PeerID[] },
  origin: "local" | "timeout" | "remote" | string,
) => void;

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
