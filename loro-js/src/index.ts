export * from "loro-wasm";
export type * from "loro-wasm";
import {
  Container,
  ContainerID,
  Delta,
  LoroDoc,
  LoroList,
  LoroMap,
  LoroText,
  LoroTree,
  LoroCounter,
  OpId,
  TreeID,
  Value,
  ContainerType,
} from "loro-wasm";

/**
 * @deprecated Please use LoroDoc
 */
export class Loro extends LoroDoc { }
export { Awareness } from "./awareness";

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
 * @prop by - How the event is triggered.
 * @prop origin - (Optional) Provides information about the origin of the event.
 * @prop diff - Contains the differential information related to the event.
 * @prop target - Identifies the container ID of the event's target.
 * @prop path - Specifies the absolute path of the event's emitter, which can be an index of a list container or a key of a map container.
 */
export interface LoroEventBatch {
  /**
   * How the event is triggered.
   *
   * - `local`: The event is triggered by a local transaction.
   * - `import`: The event is triggered by an import operation.
   * - `checkout`: The event is triggered by a checkout operation.
   */
  by: "local" | "import" | "checkout";
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
  | {
    target: TreeID;
    action: "create";
    parent: TreeID | undefined;
    index: number;
    fractionalIndex: string;
  }
  | { target: TreeID; action: "delete"; oldParent: TreeID | undefined; oldIndex: number }
  | {
    target: TreeID;
    action: "move";
    parent: TreeID | undefined;
    index: number;
    fractionalIndex: string;
    oldParent: TreeID | undefined;
    oldIndex: number;
  };

export type TreeDiff = {
  type: "tree";
  diff: TreeDiffItem[];
};

export type CounterDiff = {
  type: "counter";
  increment: number;
}

export type Diff = ListDiff | TextDiff | MapDiff | TreeDiff | CounterDiff;

interface Listener {
  (event: LoroEventBatch): void;
}

const CONTAINER_TYPES = ["Map", "Text", "List", "Tree", "MovableList", "Counter"];

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
): T extends LoroText
  ? "Text"
  : T extends LoroMap<any>
  ? "Map"
  : T extends LoroTree<any>
  ? "Tree"
  : T extends LoroList<any>
  ? "List"
  : T extends LoroCounter ? "Counter"
  : "Json" {
  if (isContainer(value)) {
    return value.kind() as unknown as any;
  }

  return "Json" as any;
}

export type Subscription = () => void;
declare module "loro-wasm" {
  interface LoroDoc {
    subscribe(listener: Listener): Subscription;
  }

  interface UndoManager {
    /**
     * Set the callback function that is called when an undo/redo step is pushed.
     * The function can return a meta data value that will be attached to the given stack item.
     *
     * @param listener - The callback function.
     */
    setOnPush(listener?: UndoConfig["onPush"]): void;
    /**
     * Set the callback function that is called when an undo/redo step is popped.
     * The function will have a meta data value that was attached to the given stack item when `onPush` was called.
     *
     * @param listener - The callback function.
     */
    setOnPop(listener?: UndoConfig["onPop"]): void;
  }

  interface LoroDoc<
    T extends Record<string, Container> = Record<string, Container>,
  > {
    /**
     * Get a LoroMap by container id
     *
     * The object returned is a new js object each time because it need to cross
     * the WASM boundary.
     *
     * @example
     * ```ts
     * import { LoroDoc } from "loro-crdt";
     *
     * const doc = new LoroDoc();
     * const map = doc.getMap("map");
     * ```
     */
    getMap<Key extends keyof T | ContainerID>(
      name: Key,
    ): T[Key] extends LoroMap ? T[Key] : LoroMap;
    /**
     * Get a LoroList by container id
     *
     * The object returned is a new js object each time because it need to cross
     * the WASM boundary.
     *
     * @example
     * ```ts
     * import { LoroDoc } from "loro-crdt";
     *
     * const doc = new LoroDoc();
     * const list = doc.getList("list");
     * ```
     */
    getList<Key extends keyof T | ContainerID>(
      name: Key,
    ): T[Key] extends LoroList ? T[Key] : LoroList;
    /**
     * Get a LoroMovableList by container id
     *
     * The object returned is a new js object each time because it need to cross
     * the WASM boundary.
     *
     * @example
     * ```ts
     * import { LoroDoc } from "loro-crdt";
     *
     * const doc = new LoroDoc();
     * const list = doc.getList("list");
     * ```
     */
    getMovableList<Key extends keyof T | ContainerID>(
      name: Key,
    ): T[Key] extends LoroMovableList ? T[Key] : LoroMovableList;
    /**
     * Get a LoroTree by container id
     *
     *  The object returned is a new js object each time because it need to cross
     *  the WASM boundary.
     *
     *  @example
     *  ```ts
     *  import { LoroDoc } from "loro-crdt";
     *
     *  const doc = new LoroDoc();
     *  const tree = doc.getTree("tree");
     *  ```
     */
    getTree<Key extends keyof T | ContainerID>(
      name: Key,
    ): T[Key] extends LoroTree ? T[Key] : LoroTree;
    getText(key: string | ContainerID): LoroText;
  }

  interface LoroList<T = unknown> {
    new(): LoroList<T>;
    /**
     *  Get elements of the list. If the value is a child container, the corresponding
     *  `Container` will be returned.
     *
     *  @example
     *  ```ts
     *  import { LoroDoc } from "loro-crdt";
     *
     *  const doc = new LoroDoc();
     *  const list = doc.getList("list");
     *  list.insert(0, 100);
     *  list.insert(1, "foo");
     *  list.insert(2, true);
     *  list.insertContainer(3, new LoroText());
     *  console.log(list.value);  // [100, "foo", true, LoroText];
     *  ```
     */
    toArray(): T[];
    /**
     * Insert a container at the index.
     *
     *  @example
     *  ```ts
     *  import { LoroDoc, LoroText } from "loro-crdt";
     *
     *  const doc = new LoroDoc();
     *  const list = doc.getList("list");
     *  list.insert(0, 100);
     *  const text = list.insertContainer(1, new LoroText());
     *  text.insert(0, "Hello");
     *  console.log(list.toJSON());  // [100, "Hello"];
     *  ```
     */
    insertContainer<C extends Container>(
      pos: number,
      child: C,
    ): T extends C ? T : C;
    /**
     * Get the value at the index. If the value is a container, the corresponding handler will be returned.
     *
     *  @example
     *  ```ts
     *  import { LoroDoc } from "loro-crdt";
     *
     *  const doc = new LoroDoc();
     *  const list = doc.getList("list");
     *  list.insert(0, 100);
     *  console.log(list.get(0));  // 100
     *  console.log(list.get(1));  // undefined
     *  ```
     */
    get(index: number): T;
    /**
     *  Insert a value at index.
     *
     *  @example
     *  ```ts
     *  import { LoroDoc } from "loro-crdt";
     *
     *  const doc = new LoroDoc();
     *  const list = doc.getList("list");
     *  list.insert(0, 100);
     *  list.insert(1, "foo");
     *  list.insert(2, true);
     *  console.log(list.value);  // [100, "foo", true];
     *  ```
     */
    insert<V extends T>(pos: number, value: Exclude<V, Container>): void;
    delete(pos: number, len: number): void;
    push<V extends T>(value: Exclude<V, Container>): void;
    subscribe(listener: Listener): Subscription;
    getAttached(): undefined | LoroList<T>;
  }

  interface LoroMovableList<T = unknown> {
    new(): LoroMovableList<T>;
    /**
     *  Get elements of the list. If the value is a child container, the corresponding
     *  `Container` will be returned.
     *
     *  @example
     *  ```ts
     *  import { LoroDoc, LoroText } from "loro-crdt";
     *
     *  const doc = new LoroDoc();
     *  const list = doc.getMovableList("list");
     *  list.insert(0, 100);
     *  list.insert(1, "foo");
     *  list.insert(2, true);
     *  list.insertContainer(3, new LoroText());
     *  console.log(list.value);  // [100, "foo", true, LoroText];
     *  ```
     */
    toArray(): T[];
    /**
     * Insert a container at the index.
     *
     *  @example
     *  ```ts
     *  import { LoroDoc } from "loro-crdt";
     *
     *  const doc = new LoroDoc();
     *  const list = doc.getMovableList("list");
     *  list.insert(0, 100);
     *  const text = list.insertContainer(1, new LoroText());
     *  text.insert(0, "Hello");
     *  console.log(list.toJSON());  // [100, "Hello"];
     *  ```
     */
    insertContainer<C extends Container>(
      pos: number,
      child: C,
    ): T extends C ? T : C;
    /**
     * Get the value at the index. If the value is a container, the corresponding handler will be returned.
     *
     *  @example
     *  ```ts
     *  import { LoroDoc } from "loro-crdt";
     *
     *  const doc = new LoroDoc();
     *  const list = doc.getMovableList("list");
     *  list.insert(0, 100);
     *  console.log(list.get(0));  // 100
     *  console.log(list.get(1));  // undefined
     *  ```
     */
    get(index: number): T;
    /**
     *  Insert a value at index.
     *
     *  @example
     *  ```ts
     *  import { LoroDoc } from "loro-crdt";
     *
     *  const doc = new LoroDoc();
     *  const list = doc.getMovableList("list");
     *  list.insert(0, 100);
     *  list.insert(1, "foo");
     *  list.insert(2, true);
     *  console.log(list.value);  // [100, "foo", true];
     *  ```
     */
    insert<V extends T>(pos: number, value: Exclude<V, Container>): void;
    delete(pos: number, len: number): void;
    push<V extends T>(value: Exclude<V, Container>): void;
    subscribe(listener: Listener): Subscription;
    getAttached(): undefined | LoroMovableList<T>;
    /**
     *  Set the value at the given position.
     *
     *  It's different from `delete` + `insert` that it will replace the value at the position.
     *
     *  For example, if you have a list `[1, 2, 3]`, and you call `set(1, 100)`, the list will be `[1, 100, 3]`.
     *  If concurrently someone call `set(1, 200)`, the list will be `[1, 200, 3]` or `[1, 100, 3]`.
     *
     *  But if you use `delete` + `insert` to simulate the set operation, they may create redundant operations
     *  and the final result will be `[1, 100, 200, 3]` or `[1, 200, 100, 3]`.
     *
     *  @example
     *  ```ts
     *  import { LoroDoc } from "loro-crdt";
     *
     *  const doc = new LoroDoc();
     *  const list = doc.getList("list");
     *  list.insert(0, 100);
     *  list.insert(1, "foo");
     *  list.insert(2, true);
     *  list.set(1, "bar");
     *  console.log(list.value);  // [100, "bar", true];
     *  ```
     */
    set<V extends T>(pos: number, value: Exclude<V, Container>): void;
    /**
     * Set a container at the index.
     *
     *  @example
     *  ```ts
     *  import { LoroDoc } from "loro-crdt";
     *
     *  const doc = new LoroDoc();
     *  const list = doc.getMovableList("list");
     *  list.insert(0, 100);
     *  const text = list.setContainer(0, new LoroText());
     *  text.insert(0, "Hello");
     *  console.log(list.toJSON());  // ["Hello"];
     *  ```
     */
    setContainer<C extends Container>(
      pos: number,
      child: C,
    ): T extends C ? T : C;
  }

  interface LoroMap<
    T extends Record<string, unknown> = Record<string, unknown>,
  > {
    new(): LoroMap<T>;
    /**
     *  Get the value of the key. If the value is a child container, the corresponding
     *  `Container` will be returned.
     *
     *  The object returned is a new js object each time because it need to cross
     *
     *  @example
     *  ```ts
     *  import { LoroDoc } from "loro-crdt";
     *
     *  const doc = new LoroDoc();
     *  const map = doc.getMap("map");
     *  map.set("foo", "bar");
     *  const bar = map.get("foo");
     *  ```
     */
    getOrCreateContainer<C extends Container>(key: string, child: C): C;
    /**
     * Set the key with a container.
     *
     *  @example
     *  ```ts
     *  import { LoroDoc } from "loro-crdt";
     *
     *  const doc = new LoroDoc();
     *  const map = doc.getMap("map");
     *  map.set("foo", "bar");
     *  const text = map.setContainer("text", new LoroText());
     *  const list = map.setContainer("list", new LoroText());
     *  ```
     */
    setContainer<C extends Container, Key extends keyof T>(
      key: Key,
      child: C,
    ): NonNullableType<T[Key]> extends C ? NonNullableType<T[Key]> : C;
    /**
     *  Get the value of the key. If the value is a child container, the corresponding
     *  `Container` will be returned.
     *
     *  The object/value returned is a new js object/value each time because it need to cross
     *  the WASM boundary.
     *
     *  @example
     *  ```ts
     *  import { LoroDoc } from "loro-crdt";
     *
     *  const doc = new LoroDoc();
     *  const map = doc.getMap("map");
     *  map.set("foo", "bar");
     *  const bar = map.get("foo");
     *  ```
     */
    get<Key extends keyof T>(key: Key): T[Key];
    /**
     * Set the key with the value.
     *
     *  If the value of the key is exist, the old value will be updated.
     *
     *  @example
     *  ```ts
     *  import { LoroDoc } from "loro-crdt";
     *
     *  const doc = new LoroDoc();
     *  const map = doc.getMap("map");
     *  map.set("foo", "bar");
     *  map.set("foo", "baz");
     *  ```
     */
    set<Key extends keyof T, V extends T[Key]>(
      key: Key,
      value: Exclude<V, Container>,
    ): void;
    delete(key: string): void;
    subscribe(listener: Listener): Subscription;
  }

  interface LoroText {
    new(): LoroText;
    insert(pos: number, text: string): void;
    delete(pos: number, len: number): void;
    subscribe(listener: Listener): Subscription;
  }

  interface LoroTree<
    T extends Record<string, unknown> = Record<string, unknown>,
  > {
    new(): LoroTree<T>;
    /**
     * Create a new tree node as the child of parent and return a `LoroTreeNode` instance.
     * If the parent is undefined, the tree node will be a root node.
     *
     * If the index is not provided, the new node will be appended to the end.
     *
     * @example
     * ```ts
     * import { LoroDoc } from "loro-crdt";
     *
     * const doc = new LoroDoc();
     * const tree = doc.getTree("tree");
     * const root = tree.createNode();
     * const node = tree.createNode(undefined, 0);
     *
     * //  undefined
     * //    /   \
     * // node  root
     * ```
     */
    createNode(parent?: TreeID, index?: number): LoroTreeNode<T>;
    move(target: TreeID, parent?: TreeID, index?: number): void;
    delete(target: TreeID): void;
    has(target: TreeID): boolean;
    /**
     * Get LoroTreeNode by the TreeID.
     */
    getNodeByID(target: TreeID): LoroTreeNode<T>;
    subscribe(listener: Listener): Subscription;
  }

  interface LoroTreeNode<
    T extends Record<string, unknown> = Record<string, unknown>,
  > {
    /**
     * Get the associated metadata map container of a tree node.
     */
    readonly data: LoroMap<T>;
    /** 
     * Create a new node as the child of the current node and
     * return an instance of `LoroTreeNode`.
     *
     * If the index is not provided, the new node will be appended to the end.
     *
     * @example
     * ```typescript
     * import { LoroDoc } from "loro-crdt";
     *
     * let doc = new LoroDoc();
     * let tree = doc.getTree("tree");
     * let root = tree.createNode();
     * let node = root.createNode();
     * let node2 = root.createNode(0);
     * //    root
     * //    /  \
     * // node2 node
     * ```
     */
    createNode(index?: number): LoroTreeNode<T>;
    move(parent?: LoroTreeNode<T>, index?: number): void;
    parent(): LoroTreeNode<T> | undefined;
    /**
     * Get the children of this node.
     *
     * The objects returned are new js objects each time because they need to cross
     * the WASM boundary.
     */
    children(): Array<LoroTreeNode<T>> | undefined;
  }

  interface AwarenessWasm<T extends Value = Value> {
    getState(peer: PeerID): T | undefined;
    getTimestamp(peer: PeerID): number | undefined;
    getAllStates(): Record<PeerID, T>;
    setLocalState(value: T): void;
    removeOutdated(): PeerID[];
  }
}

type NonNullableType<T> = Exclude<T, null | undefined>;

export function newContainerID(id: OpId, type: ContainerType): ContainerID {
  return `cid:${id.counter}@${id.peer}:${type}`;
}

export function newRootContainerID(name: string, type: ContainerType): ContainerID {
  return `cid:root-${name}:${type}`;
}
