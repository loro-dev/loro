/**
 * The subset of the `loro-crdt` API that the differential tests drive. Both the
 * Rust WASM build and loro.js implement it, so one driver runs against both.
 */

export interface OpIdLike {
  readonly peer: string;
  readonly counter: number;
}

export type FrontiersLike = OpIdLike[];

export interface VersionVectorLike {
  toJSON(): Map<string, number>;
}

export interface ContainerLike {
  readonly id: string;
  kind(): string;
  isAttached(): boolean;
  getShallowValue(): unknown;
  toJSON(): unknown;
}

export interface SequenceLike extends ContainerLike {
  readonly length: number;
  insert(pos: number, value: unknown): void;
  delete(pos: number, len: number): void;
  insertContainer(pos: number, child: ContainerLike): ContainerLike;
}

export interface MovableListLike extends SequenceLike {
  move(from: number, to: number): void;
  set(pos: number, value: unknown): void;
  setContainer(pos: number, child: ContainerLike): ContainerLike;
  getCreatorAt(pos: number): string | undefined;
  getLastEditorAt(pos: number): string | undefined;
  getLastMoverAt(pos: number): string | undefined;
  getCursor(pos: number, side?: -1 | 0 | 1): CursorLike | undefined;
}

export interface MapLike extends ContainerLike {
  set(key: string, value: unknown): void;
  delete(key: string): void;
  setContainer(key: string, child: ContainerLike): ContainerLike;
}

export interface TextLike extends ContainerLike {
  readonly length: number;
  insert(pos: number, text: string): void;
  delete(pos: number, len: number): void;
}

export interface CursorLike {
  encode(): Uint8Array;
}

export interface EventLike {
  readonly target: string;
  readonly path: readonly (string | number)[];
  readonly diff: unknown;
}

export interface EventBatchLike {
  readonly by: string;
  readonly origin?: string | undefined;
  readonly events: readonly EventLike[];
}

export interface DocLike {
  setPeerId(peer: number | bigint | string): void;
  getMovableList(name: string): MovableListLike;
  getMap(name: string): MapLike;
  getContainerById(id: string): ContainerLike | undefined;
  commit(options?: { readonly origin?: string; readonly message?: string }): void;
  getPendingTxnLength(): number;
  export(mode: unknown): Uint8Array;
  import(bytes: Uint8Array): unknown;
  importBatch(bytes: Uint8Array[]): unknown;
  toJSON(): unknown;
  getShallowValue(): Record<string, string>;
  oplogVersion(): VersionVectorLike;
  frontiers(): FrontiersLike;
  oplogFrontiers(): FrontiersLike;
  checkout(frontiers: FrontiersLike): void;
  checkoutToLatest(): void;
  isDetached(): boolean;
  revertTo(frontiers: FrontiersLike): void;
  diff(from: FrontiersLike, to: FrontiersLike, forJson: boolean): [string, unknown][];
  subscribe(listener: (event: EventBatchLike) => void): () => void;
  isShallow(): boolean;
  shallowSinceVV(): VersionVectorLike;
  getCursorPos(cursor: CursorLike): { readonly offset: number };
  forkAt(frontiers: FrontiersLike): DocLike;
}

export interface UndoManagerLike {
  undo(): boolean;
  redo(): boolean;
}

export interface EngineModule {
  readonly LoroDoc: new () => DocLike;
  readonly LoroMap: new () => MapLike;
  readonly LoroText: new () => TextLike;
  readonly LoroMovableList: new () => MovableListLike;
  readonly VersionVector: new (input: Map<string, number>) => VersionVectorLike;
  readonly UndoManager: new (
    doc: DocLike,
    config: { readonly mergeInterval?: number; readonly maxUndoSteps?: number },
  ) => UndoManagerLike;
  readonly Cursor: { decode(bytes: Uint8Array): CursorLike };
}
