export * from "loro-wasm";
import {
  Container,
  ContainerID,
  Delta,
  Loro,
  LoroList,
  LoroMap,
  LoroText,
  LoroTree,
  OpId,
  TreeID,
  Value,
} from "loro-wasm";

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
): T extends LoroText ? "Text"
  : T extends LoroMap<any> ? "Map"
  : T extends LoroTree<any> ? "Tree"
  : T extends LoroList<any> ? "List"
  : "Json" {
  if (isContainer(value)) {
    return value.kind() as unknown as any;
  }

  return "Json" as any;
}

declare module "loro-wasm" {
  interface Loro {
    subscribe(listener: Listener): number;
  }

  interface Loro<
    T extends Record<string, Container> = Record<string, Container>,
  > {
    getMap<Key extends keyof T>(
      name: Key,
    ): T[Key] extends LoroMap ? T[Key] : LoroMap;
    getList<Key extends keyof T>(
      name: Key,
    ): T[Key] extends LoroList ? T[Key] : LoroList;
    getTree<Key extends keyof T>(
      name: Key,
    ): T[Key] extends LoroTree ? T[Key] : LoroTree;
    getText(key: string | ContainerID): LoroText;
  }

  interface LoroList<T = unknown> {
    new (): LoroList<T>;
    toArray(): T[];
    insertContainer<C extends Container>(
      pos: number,
      child: C,
    ): T extends C ? T : C;
    get(index: number): T;
    insert(pos: number, value: Exclude<T, Container>): void;
    delete(pos: number, len: number): void;
    subscribe(txn: Loro, listener: Listener): number;
    getAttached(): undefined | LoroList<T>;
  }

  interface LoroMap<
    T extends Record<string, unknown> = Record<string, unknown>,
  > {
    new (): LoroMap<T>;
    getOrCreateContainer<C extends Container>(key: string, child: C): C;
    setContainer<C extends Container, Key extends keyof T>(
      key: Key,
      child: C,
    ): NonNullableType<T[Key]> extends C ? NonNullableType<T[Key]> : C;
    get<Key extends keyof T>(key: Key): T[Key];
    set<Key extends keyof T>(key: Key, value: Exclude<T[Key], Container>): void;
    delete(key: string): void;
    subscribe(txn: Loro, listener: Listener): number;
  }

  interface LoroText {
    new (): LoroText;
    insert(pos: number, text: string): void;
    delete(pos: number, len: number): void;
    subscribe(txn: Loro, listener: Listener): number;
  }

  interface LoroTree<
    T extends Record<string, unknown> = Record<string, unknown>,
  > {
    new (): LoroTree<T>;
    createNode(parent: TreeID | undefined): LoroTreeNode<T>;
    move(target: TreeID, parent: TreeID | undefined): void;
    delete(target: TreeID): void;
    has(target: TreeID): boolean;
    getNodeByID(target: TreeID): LoroTreeNode;
    subscribe(txn: Loro, listener: Listener): number;
  }

  interface LoroTreeNode<
    T extends Record<string, unknown> = Record<string, unknown>,
  > {
    readonly data: LoroMap<T>;
    createNode(): LoroTreeNode<T>;
    setAsRoot(): void;
    moveTo(parent: LoroTreeNode<T>): void;
    parent(): LoroTreeNode<T> | undefined;
    children(): Array<LoroTreeNode<T>>;
  }
}

type NonNullableType<T> = Exclude<T, null | undefined>;
