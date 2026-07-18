import type { VersionVector } from "./version-vector";
import type { Cursor } from "./containers";

export type ContainerType = "Text" | "Map" | "List" | "Tree" | "MovableList" | "Counter";
export type PeerID = `${number}`;
export type PeerIdInput = string | number | bigint;
export type TextPosType = "unicode" | "utf16" | "utf8";
export type TextStyleExpand = "before" | "after" | "none" | "both";
export interface TextStyleConfig {
  readonly expand: TextStyleExpand;
}
export type ContainerID =
  | `cid:root-${string}:${ContainerType}`
  | `cid:${number}@${PeerID}:${ContainerType}`;
export type JsonContainerID = `🦜:${ContainerID}`;
export type TreeID = `${number}@${PeerID}`;

export interface OpId {
  readonly peer: PeerID;
  readonly counter: number;
}

export type Frontiers = OpId[];

export type Value =
  | string
  | number
  | boolean
  | null
  | { readonly [key: string]: Value }
  | Uint8Array
  | readonly Value[]
  | undefined;

export type Delta<T> =
  | { readonly insert: T; readonly attributes?: Readonly<Record<string, Value>> }
  | { readonly delete: number }
  | { readonly retain: number; readonly attributes?: Readonly<Record<string, Value>> };

export type ExportMode =
  | { readonly mode: "update"; readonly from?: VersionVector }
  | { readonly mode: "snapshot" }
  | { readonly mode: "shallow-snapshot"; readonly frontiers: Frontiers }
  | {
      readonly mode: "updates-in-range";
      readonly spans: readonly { readonly id: OpId; readonly len: number }[];
    };

export interface CommitOptions {
  readonly origin?: string;
  readonly timestamp?: number;
  readonly message?: string;
}

export interface Change {
  readonly peer: PeerID;
  readonly counter: number;
  readonly lamport: number;
  readonly length: number;
  readonly timestamp: number;
  readonly deps: OpId[];
  readonly message: string | undefined;
}

export type JsonOpID = `${number}@${PeerID}`;
export type JsonValue =
  | string
  | number
  | boolean
  | null
  | Uint8Array
  | JsonValue[]
  | { readonly [key: string]: JsonValue };

export type JsonOpContent =
  | { readonly type: "insert"; readonly pos: number; readonly text: string }
  | {
      readonly type: "delete";
      readonly pos: number;
      readonly len: number;
      readonly start_id: JsonOpID;
    }
  | {
      readonly type: "mark";
      readonly start: number;
      readonly end: number;
      readonly style_key: string;
      readonly style_value: JsonValue;
      readonly info: number;
    }
  | { readonly type: "mark_end" }
  | { readonly type: "insert"; readonly key: string; readonly value: JsonValue }
  | { readonly type: "delete"; readonly key: string }
  | { readonly type: "insert"; readonly pos: number; readonly value: JsonValue[] }
  | {
      readonly type: "move";
      readonly from: number;
      readonly to: number;
      readonly elem_id: JsonOpID;
    }
  | { readonly type: "set"; readonly elem_id: JsonOpID; readonly value: JsonValue }
  | {
      readonly type: "create" | "move";
      readonly target: TreeID;
      readonly parent: TreeID | null | undefined;
      readonly fractional_index?: string;
    }
  | { readonly type: "delete"; readonly target: TreeID }
  | { readonly type: "counter"; readonly value: number; readonly prop: number }
  | {
      readonly type: "unknown";
      readonly prop: number;
      readonly value_type?: "unknown";
      readonly value: unknown;
    };

export interface JsonOp {
  readonly container: ContainerID;
  readonly counter: number;
  readonly content: JsonOpContent;
}

export interface JsonChange {
  readonly id: JsonOpID;
  readonly timestamp: number;
  readonly deps: JsonOpID[];
  readonly lamport: number;
  readonly msg: string | null | undefined;
  readonly ops: JsonOp[];
}

export interface JsonSchema {
  readonly schema_version: 1;
  readonly start_version: Readonly<Record<string, number>>;
  readonly peers: PeerID[] | null;
  readonly changes: JsonChange[];
}

export interface ImportBlobMetadata {
  readonly partialStartVersionVector: VersionVector;
  readonly partialEndVersionVector: VersionVector;
  readonly startFrontiers: Frontiers;
  readonly startTimestamp: number;
  readonly endTimestamp: number;
  readonly mode:
    | "outdated-snapshot"
    | "outdated-update"
    | "snapshot"
    | "shallow-snapshot"
    | "update";
  readonly changeNum: number;
}

export interface UndoItemValue {
  readonly value: Value;
  readonly cursors: Cursor[];
}

export interface UndoConfig {
  readonly mergeInterval?: number;
  readonly maxUndoSteps?: number;
  readonly excludeOriginPrefixes?: string[];
  readonly onPush?: (
    isUndo: boolean,
    counterRange: CounterSpan,
    event?: LoroEventBatch,
  ) => UndoItemValue | undefined;
  readonly onPop?: (
    isUndo: boolean,
    value: UndoItemValue,
    counterRange: CounterSpan,
  ) => void;
}

export interface CounterSpan {
  readonly start: number;
  readonly end: number;
}

export interface IdSpan {
  readonly peer: PeerID;
  readonly counter: number;
  readonly length: number;
}

export interface VersionVectorDiff {
  readonly retreat: IdSpan[];
  readonly forward: IdSpan[];
}

export interface ImportStatus {
  readonly success: Map<PeerID, CounterSpan>;
  readonly pending: Map<PeerID, CounterSpan> | null;
}

export type Side = -1 | 0 | 1;
export type Path = (number | string)[];

export type ListDiff = { readonly type: "list"; readonly diff: Delta<unknown[]>[] };
export type TextDiff = { readonly type: "text"; readonly diff: Delta<string>[] };
export type MapDiff = {
  readonly type: "map";
  readonly updated: Readonly<Record<string, unknown>>;
};
export type TreeDiffItem =
  | {
      readonly target: TreeID;
      readonly action: "create";
      readonly parent: TreeID | undefined;
      readonly index: number;
      readonly fractionalIndex: string;
    }
  | {
      readonly target: TreeID;
      readonly action: "delete";
      readonly oldParent: TreeID | undefined;
      readonly oldIndex: number;
    }
  | {
      readonly target: TreeID;
      readonly action: "move";
      readonly parent: TreeID | undefined;
      readonly index: number;
      readonly fractionalIndex: string;
      readonly oldParent: TreeID | undefined;
      readonly oldIndex: number;
    };
export type TreeDiff = { readonly type: "tree"; readonly diff: TreeDiffItem[] };
export type CounterDiff = { readonly type: "counter"; readonly increment: number };
export type Diff = ListDiff | TextDiff | MapDiff | TreeDiff | CounterDiff;
export type ListJsonDiff = {
  readonly type: "list";
  readonly diff: Delta<Value[]>[];
};
export type MapJsonDiff = {
  readonly type: "map";
  readonly updated: Readonly<Record<string, Value | undefined>>;
};
export type JsonDiff = ListJsonDiff | TextDiff | MapJsonDiff | TreeDiff | CounterDiff;
export type DiffBatch = [ContainerID, Diff | JsonDiff][];

export interface LoroEvent {
  readonly target: ContainerID;
  readonly diff: Diff;
  readonly path: Path;
}

export interface LoroEventBatch {
  readonly by: "local" | "import" | "checkout";
  readonly origin?: string;
  readonly currentTarget?: ContainerID;
  readonly events: LoroEvent[];
  readonly from: Frontiers;
  readonly to: Frontiers;
}

export type Subscription = () => void;

export interface TreeNodeShallowValue {
  readonly id: TreeID;
  readonly parent: TreeID | null;
  readonly index: number;
  readonly fractional_index: string;
  readonly meta: ContainerID;
  readonly children: TreeNodeShallowValue[];
}

export interface TreeNodeValue<T = Record<string, unknown>> {
  readonly id: TreeID;
  readonly parent: TreeID | undefined;
  readonly index: number;
  readonly fractionalIndex: string;
  readonly meta: T;
  readonly children: TreeNodeValue<T>[];
}

export type TreeNodeJSON<T = Record<string, unknown>> = TreeNodeValue<T>;

export interface TextUpdateOptions {
  readonly timeoutMs?: number;
  readonly useRefinedDiff?: boolean;
}
