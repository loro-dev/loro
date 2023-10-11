export {
  LoroList,
  LoroMap,
  LoroText,
  LoroTree,
  PrelimList,
  PrelimMap,
  PrelimText,
  setPanicHook,
  Transaction,
} from "loro-wasm";
import { PrelimMap } from "loro-wasm";
import { PrelimText } from "loro-wasm";
import { PrelimList } from "loro-wasm";
import {
  ContainerID,
  TreeID,
  Loro,
  LoroList,
  LoroMap,
  LoroText,
  LoroTree,
  Transaction,
} from "loro-wasm";

export type { ContainerID, ContainerType, TreeID } from "loro-wasm";

Loro.prototype.transact = function (cb, origin) {
  return this.__raw__transactionWithOrigin(origin || "", (txn: Transaction) => {
    try {
      return cb(txn);
    } finally {
      txn.free();
    }
  });
};

Loro.prototype.getTypedMap = function (...args) { return this.getMap(...args) };
Loro.prototype.getTypedList = function (...args) { return this.getList(...args) };
LoroList.prototype.getTyped = function (loro, index) {
  const value = this.get(index);
  if (typeof value === "string" && isContainerId(value)) {
    return loro.getContainerById(value);
  } else {
    return value;
  }
};
LoroList.prototype.insertTyped = function (...args) {
  return this.insert(...args)
}
LoroMap.prototype.getTyped = function (loro, key) {
  const value = this.get(key);
  if (typeof value === "string" && isContainerId(value)) {
    return loro.getContainerById(value);
  } else {
    return value;
  }
};
LoroMap.prototype.setTyped = function (...args) { return this.set(...args) };

LoroText.prototype.insert = function (txn, pos, text) {
  this.__txn_insert(txn, pos, text);
};

LoroText.prototype.delete = function (txn, pos, len) {
  this.__txn_delete(txn, pos, len);
};

LoroList.prototype.insert = function (txn, pos, len) {
  this.__txn_insert(txn, pos, len);
};

LoroList.prototype.delete = function (txn, pos, len) {
  this.__txn_delete(txn, pos, len);
};

LoroMap.prototype.set = function (txn, key, value) {
  this.__txn_insert(txn, key, value);
};

LoroMap.prototype.delete = function (txn, key) {
  this.__txn_delete(txn, key);
};

LoroTree.prototype.create = function(txn){
  return this.__txn_create(txn);
}

LoroTree.prototype.createChild = function(txn, id){
  return this.__txn_create_children(txn, id)
}

LoroTree.prototype.move = function(txn, target, parent){
  this.__txn_move(txn, target, parent)
}

LoroTree.prototype.delete = function(txn, target){
  this.__txn_delete(txn, target)
}

LoroTree.prototype.insertMeta = function(txn, target, key, value){
  this.__txn_insert_meta(txn, target, key, value)
}

LoroTree.prototype.getMeta = function(txn, target, key){
  return this.__txn_get_meta(txn, target, key)
}

export type Value =
  | ContainerID
  | string
  | number
  | null
  | boolean
  | { [key: string]: Value }
  | Uint8Array
  | Value[];

export type Prelim = PrelimList | PrelimMap | PrelimText;

export type Path = (number | string)[];
export type Delta<T> = {
  type: "insert";
  value: T;
} | {
  type: "delete";
  len: number;
} | {
  type: "retain";
  len: number;
};

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
  diff: {
    target: TreeID,
    action: {type: "create"} | {type: "move", parent: TreeID} | {type: "delete"}
  }[]
}

export type Diff = ListDiff | TextDiff | MapDiff| TreeDiff;

export interface LoroEvent {
  local: boolean;
  origin?: string;
  diff: Diff;
  target: ContainerID;
  path: Path;
}

interface Listener {
  (event: LoroEvent): void;
}

const CONTAINER_TYPES = ["Map", "Text", "List", "Tree"];

export function isContainerId(s: string): s is ContainerID {
  try {
    if (s.startsWith("/")) {
      const [_, type] = s.slice(1).split(":");
      if (!CONTAINER_TYPES.includes(type)) {
        return false;
      }
    } else {
      const [id, type] = s.split(":");
      if (!CONTAINER_TYPES.includes(type)) {
        return false;
      }

      const [counter, client] = id.split("@");
      Number.parseInt(counter);
      Number.parseInt(client);
    }

    return true;
  } catch (e) {
    return false;
  }
}

export { Loro };

declare module "loro-wasm" {
  interface Loro {
    subscribe(listener: Listener): number;
    transact<T>(f: (tx: Transaction) => T, origin?: string): T;
  }

  interface Loro<T extends Record<string, any> = Record<string, any>> {
    getTypedMap<Key extends (keyof T) & string>(
      name: Key,
    ): T[Key] extends LoroMap ? T[Key] : never;
    getTypedList<Key extends (keyof T) & string>(
      name: Key,
    ): T[Key] extends LoroList ? T[Key] : never;
  }

  interface LoroList<T extends any[] = any[]> {
    insertContainer(
      txn: Transaction,
      pos: number,
      container: "Map",
    ): LoroMap;
    insertContainer(
      txn: Transaction,
      pos: number,
      container: "List",
    ): LoroList;
    insertContainer(
      txn: Transaction,
      pos: number,
      container: "Text",
    ): LoroText;
    insertContainer(
      txn: Transaction,
      pos: number,
      container: string,
    ): never;

    get(index: number): Value;
    getTyped<Key extends (keyof T) & number>(loro: Loro, index: Key): T[Key];
    insertTyped<Key extends (keyof T) & number>(
      txn: Transaction,
      pos: Key,
      value: T[Key],
    ): void;
    insert(txn: Transaction, pos: number, value: Value | Prelim): void;
    delete(txn: Transaction, pos: number, len: number): void;
    subscribe(txn: Loro, listener: Listener): number;
  }

  interface LoroMap<T extends Record<string, any> = Record<string, any>> {
    insertContainer(
      txn: Transaction,
      key: string,
      container_type: "Map",
    ): LoroMap;
    insertContainer(
      txn: Transaction,
      key: string,
      container_type: "List",
    ): LoroList;
    insertContainer(
      txn: Transaction,
      key: string,
      container_type: "Text",
    ): LoroText;
    insertContainer(
      txn: Transaction,
      key: string,
      container_type: string,
    ): never;

    get(key: string): Value;
    getTyped<Key extends (keyof T) & string>(
      txn: Loro,
      key: Key,
    ): T[Key];
    set(txn: Transaction, key: string, value: Value | Prelim): void;
    setTyped<Key extends (keyof T) & string>(
      txn: Transaction,
      key: Key,
      value: T[Key],
    ): void;
    delete(txn: Transaction, key: string): void;
    subscribe(txn: Loro, listener: Listener): number;
  }

  interface LoroText {
    insert(txn: Transaction, pos: number, text: string): void;
    delete(txn: Transaction, pos: number, len: number): void;
    subscribe(txn: Loro, listener: Listener): number;
  }

  interface TreeNode{
    id: string,
    parent: string | undefined,
    children: TreeNode[]
    meta: {[key: string]: any}
  }

  interface LoroTree{
    create(txn: Transaction): TreeID;
    createChild(txn: Transaction, parent: TreeID): TreeID;
    delete(txn: Transaction, target: TreeID):void;
    move(txn: Transaction, target: TreeID, parent: TreeID):void;
    insertMeta(txn: Transaction, target: TreeID, key: string, value: Value | Prelim):void;
    getMeta(txn: Transaction, target: TreeID, key: string):Value;
    subscribe(txn: Loro, listener: Listener): number;
    getDeepValue(): {roots: TreeNode[]};
  }
}
