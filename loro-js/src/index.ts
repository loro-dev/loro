export * from "loro-wasm";
import { Container, ContainerType, Delta, OpId, Value } from "loro-wasm";
import { PrelimText, PrelimList, PrelimMap } from "loro-wasm";
import {
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

export type Prelim = PrelimList | PrelimMap | PrelimText;
export type Frontiers = OpId[];

/**
 * Represents a path to identify the exact location of an event's target.
 * The path is composed of numbers (e.g., indices of a list container) and strings
 * (e.g., keys of a map container), indicating the absolute position of the event's source
 * within a loro document.
 */
export type Path = (number | string)[];

/**
 * The event of Loro.
 * @prop local - Indicates whether the event is local.
 * @prop origin - (Optional) Provides information about the origin of the event.
 * @prop diff - Contains the differential information related to the event.
 * @prop target - Identifies the container ID of the event's target.
 * @prop path - Specifies the absolute path of the event's emitter, which can be an index of a list container or a key of a map container.
 */
export interface LoroEvent {
  /**
   * The unique ID of the event.
   */
  id: bigint;
  local: boolean;
  origin?: string;
  /**
   * If true, this event was triggered by a child container.
   */
  fromChildren: boolean;
  /**
   * If true, this event was triggered by a checkout.
   */
  fromCheckout: boolean;
  diff: Diff;
  target: ContainerID;
  path: Path;
}

export type ListDiff = {
  type: "list";
  diff: Delta<Value[]>[];
};

export type TextDiff = {
  type: "text";
  diff: Delta<string>[];
};

export type MapDiff = {
  type: "map";
  updated: Record<string, Value | undefined>;
};

export type TreeDiff = {
  type: "tree";
  diff:
  | { target: TreeID; action: "create" | "delete" }
  | { target: TreeID; action: "move"; parent: TreeID };
};

export type Diff = ListDiff | TextDiff | MapDiff | TreeDiff;

interface Listener {
  (event: LoroEvent): void;
}

const CONTAINER_TYPES = ["Map", "Text", "List", "Tree"];

export function isContainerId(s: string): s is ContainerID {
  return s.startsWith("cid:");
}

export { Loro };

export function isContainer(value: any): value is Container {
  if (typeof value !== "object" || value == null) {
    return false;
  }

  const p = value.__proto__;
  return p.hasOwnProperty("kind") && CONTAINER_TYPES.includes(value.kind());
}

export function valueType(value: any): "Json" | ContainerType {
  if (isContainer(value)) {
    return value.kind();
  }

  return "Json";
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
    insert(pos: number, value: Value | Prelim): void;
    delete(pos: number, len: number): void;
    subscribe(txn: Loro, listener: Listener): number;
  }

  interface LoroMap<T extends Record<string, any> = Record<string, any>> {
    setContainer(key: string, container_type: "Map"): LoroMap;
    setContainer(key: string, container_type: "List"): LoroList;
    setContainer(key: string, container_type: "Text"): LoroText;
    setContainer(key: string, container_type: "Tree"): LoroTree;
    setContainer(key: string, container_type: string): never;

    get(key: string): undefined | Value | Container;
    getTyped<Key extends keyof T & string>(txn: Loro, key: Key): T[Key];
    set(key: string, value: Value | Prelim): void;
    setTyped<Key extends keyof T & string>(key: Key, value: T[Key]): void;
    delete(key: string): void;
    subscribe(txn: Loro, listener: Listener): number;
  }

  interface LoroText {
    insert(pos: number, text: string): void;
    delete(pos: number, len: number): void;
    subscribe(txn: Loro, listener: Listener): number;
  }
}
