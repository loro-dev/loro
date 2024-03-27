export * from "loro-wasm";
import {
  Container,
  Delta,
  LoroText,
  LoroTree,
  LoroTreeNode,
  OpId,
  Value,
  ContainerID,
  Loro,
  LoroList,
  LoroMap,
  TreeID,
} from "loro-wasm";

Loro.prototype.getTypedMap = function (...args) {
  return this.getMap(...args);
};
Loro.prototype.getTypedList = function (...args) {
  return this.getList(...args);
};
LoroList.prototype.getTyped = function (loro, index) {
  const value = this.get(index);
  if (typeof value === "string" && isContainerId(value)) {
    return loro.getContainerById(value);
  } else {
    return value;
  }
};
LoroList.prototype.insertTyped = function (...args) {
  return this.insert(...args);
};
LoroMap.prototype.getTyped = function (loro, key) {
  const value = this.get(key);
  if (typeof value === "string" && isContainerId(value)) {
    return loro.getContainerById(value);
  } else {
    return value;
  }
};
LoroMap.prototype.setTyped = function (...args) {
  return this.set(...args);
};

export type Frontiers = OpId[];

/**
 * Represents a path to identify the exact location of an event's target.
 * The path is composed of numbers (e.g., indices of a list container) strings
 * (e.g., keys of a map container) and TreeID (the node of a tree container),
 * indicating the absolute position of the event's source within a loro document.
 */
export type Path = (number | string | TreeID)[];

/**
 * A batch of events that created by a single `import`/`transaction`/`checkout`.
 *
 * @prop local - Indicates whether the event is local.
 * @prop origin - (Optional) Provides information about the origin of the event.
 * @prop diff - Contains the differential information related to the event.
 * @prop target - Identifies the container ID of the event's target.
 * @prop path - Specifies the absolute path of the event's emitter, which can be an index of a list container or a key of a map container.
 */
export interface LoroEventBatch {
  local: boolean;
  /**
   * If true, this event was triggered by a checkout.
   */
  fromCheckout: boolean;
  origin?: string;
  /**
   * The container ID of the current event receiver.
   * It's undefined if the subscriber is on the root document.
   */
  currentTarget?: ContainerID;
  events: LoroEvent[];
}

/**
 * The concrete event of Loro.
 */
export interface LoroEvent {
  /**
   * The container ID of the event's target.
   */
  target: ContainerID;
  diff: Diff;
  /**
   * The absolute path of the event's emitter, which can be an index of a list container or a key of a map container.
   */
  path: Path;
}

export type ListDiff = {
  type: "list";
  diff: Delta<(Value | Container)[]>[];
};

export type TextDiff = {
  type: "text";
  diff: Delta<string>[];
};

export type MapDiff = {
  type: "map";
  updated: Record<string, Value | Container | undefined>;
};

export type TreeDiffItem =
  | { target: TreeID; action: "create"; parent: TreeID | undefined }
  | { target: TreeID; action: "delete" }
  | { target: TreeID; action: "move"; parent: TreeID | undefined };

export type TreeDiff = {
  type: "tree";
  diff: TreeDiffItem[];
};

export type Diff = ListDiff | TextDiff | MapDiff | TreeDiff;

interface Listener {
  (event: LoroEventBatch): void;
}

const CONTAINER_TYPES = ["Map", "Text", "List", "Tree"];

export function isContainerId(s: string): s is ContainerID {
  return s.startsWith("cid:");
}

export { Loro };

/**  Whether the value is a container.
 *
 * # Example
 *
 * ```ts
 * const doc = new Loro();
 * const map = doc.getMap("map");
 * const list = doc.getList("list");
 * const text = doc.getText("text");
 * isContainer(map); // true
 * isContainer(list); // true
 * isContainer(text); // true
 * isContainer(123); // false
 * isContainer("123"); // false
 * isContainer({}); // false
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
 * const doc = new Loro();
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
  : T extends LoroMap
  ? "Map"
  : T extends LoroTree
  ? "Tree"
  : T extends LoroList
  ? "List"
  : "Json" {
  if (isContainer(value)) {
    return value.kind();
  }

  return "Json" as any;
}

declare module "loro-wasm" {
  interface Loro {
    subscribe(listener: Listener): number;
  }

  interface Loro<T extends Record<string, any> = Record<string, any>> {
    getTypedMap<Key extends keyof T & string>(
      name: Key,
    ): T[Key] extends LoroMap ? T[Key] : never;
    getTypedList<Key extends keyof T & string>(
      name: Key,
    ): T[Key] extends LoroList ? T[Key] : never;
  }

  interface LoroList<T extends any[] = any[]> {
    insertContainer(pos: number, container: "Map"): LoroMap;
    insertContainer(pos: number, container: "List"): LoroList;
    insertContainer(pos: number, container: "Text"): LoroText;
    insertContainer(pos: number, container: "Tree"): LoroTree;
    insertContainer(pos: number, container: string): never;

    get(index: number): undefined | Value | Container;
    getTyped<Key extends keyof T & number>(loro: Loro, index: Key): T[Key];
    insertTyped<Key extends keyof T & number>(pos: Key, value: T[Key]): void;
    insert(pos: number, value: Value): void;
    delete(pos: number, len: number): void;
    subscribe(txn: Loro, listener: Listener): number;
  }

  interface LoroMap<T extends Record<string, any> = Record<string, any>> {
    getOrCreateContainer(key: string, container_type: "Map"): LoroMap;
    getOrCreateContainer(key: string, container_type: "List"): LoroList;
    getOrCreateContainer(key: string, container_type: "Text"): LoroText;
    getOrCreateContainer(key: string, container_type: "Tree"): LoroTree;
    getOrCreateContainer(key: string, container_type: string): never;

    setContainer(key: string, container_type: "Map"): LoroMap;
    setContainer(key: string, container_type: "List"): LoroList;
    setContainer(key: string, container_type: "Text"): LoroText;
    setContainer(key: string, container_type: "Tree"): LoroTree;
    setContainer(key: string, container_type: string): never;

    get(key: string): undefined | Value | Container;
    getTyped<Key extends keyof T & string>(txn: Loro, key: Key): T[Key];
    set(key: string, value: Value): void;
    setTyped<Key extends keyof T & string>(key: Key, value: T[Key]): void;
    delete(key: string): void;
    subscribe(txn: Loro, listener: Listener): number;
  }

  interface LoroText {
    insert(pos: number, text: string): void;
    delete(pos: number, len: number): void;
    subscribe(txn: Loro, listener: Listener): number;
  }

  interface LoroTree {
    createNode(parent: TreeID | undefined): LoroTreeNode;
    move(target: TreeID, parent: TreeID | undefined): void;
    delete(target: TreeID): void;
    has(target: TreeID): boolean;
    getNodeByID(target: TreeID): LoroTreeNode;
    subscribe(txn: Loro, listener: Listener): number;
  }

  interface LoroTreeNode {
    readonly data: LoroMap;
    createNode(): LoroTreeNode;
    setAsRoot(): void;
    moveTo(parent: LoroTreeNode): void;
    parent(): LoroTreeNode | undefined;
    children(): Array<LoroTreeNode>;
  }
}
