import { bytesToHex } from "../codec/bytes";
import {
  containerTypeFromHistoricalByte,
  containerTypeToHistoricalByte,
} from "../codec/container-id";
import { PostcardReader, PostcardWriter } from "../codec/postcard";
import type { ContainerId as CodecContainerId, Id as CodecId } from "../codec/types";
import { formatContainerId, formatTreeId, parseContainerId } from "./ids";
import type { LoroDoc } from "./document";
import { fractionalIndexBetween } from "./fractional-index";
import { OrderedIndex } from "./ordered-index";
import { SequenceIndex, SequenceSpan } from "./sequence-index";
import type {
  SequenceIdRun,
  SequenceInsertionIdContext,
  SequenceStorage,
} from "./sequence-index";
import { TextStyleIndex } from "./text-style-index";
import type {
  ContainerID,
  ContainerType,
  Delta,
  LoroEventBatch,
  Side,
  Subscription,
  TextPosType,
  TextUpdateOptions,
  TreeID,
  TreeNodeJSON,
  TreeNodeShallowValue,
  TreeNodeValue,
  Value,
} from "./types";

export type RuntimeValue =
  | string
  | number
  | boolean
  | null
  | Uint8Array
  | RuntimeValue[]
  | { [key: string]: RuntimeValue }
  | LoroContainer;

export interface LastWriter {
  readonly peer: bigint;
  readonly lamport: number;
}

export interface SequenceElement {
  readonly id: CodecId;
  lamport: number;
  value: RuntimeValue;
  deleted: boolean;
  originLeft: CodecId | undefined;
  originRight: CodecId | undefined;
  deletedBy?: CodecId[] | undefined;
  deletedByPeer?: bigint | undefined;
  deletedByCounter?: number | undefined;
  valueHistory?: SequenceValueMeta[] | undefined;
  moveHistory?: SequenceMoveMeta[] | undefined;
}

export interface SequenceValueMeta {
  readonly id: CodecId;
  readonly lamport: number;
  readonly value: RuntimeValue;
}

export interface SequenceMoveMeta {
  readonly id: CodecId;
  readonly lamport: number;
  readonly beforePrevious: CodecId | undefined;
  readonly beforeNext: CodecId | undefined;
  readonly afterPrevious: CodecId | undefined;
  readonly afterNext: CodecId | undefined;
}

export type CausalVersion = ReadonlyMap<bigint, number>;

interface ParentLink {
  readonly container: LoroContainer;
  readonly binding?:
    | { readonly kind: "map"; readonly key: string }
    | { readonly kind: "sequence"; readonly element: SequenceElement }
    | { readonly kind: "tree"; readonly record: TreeNodeRecord };
}

export abstract class LoroContainer {
  _doc: LoroDoc | undefined;
  _codecId: CodecContainerId | undefined;
  _containerId: ContainerID | undefined;
  _parentLink: ParentLink | undefined;
  _attached: LoroContainer | undefined;

  abstract kind(): ContainerType;
  abstract toJSON(): unknown;
  abstract _reset(): void;

  free(): void {}

  get id(): ContainerID {
    return this._containerId ?? (`cid:0@0:${this.kind()}` as ContainerID);
  }

  isAttached(): boolean {
    return this._doc !== undefined;
  }

  getAttached(): this | undefined {
    return (this.isAttached() ? this : this._attached) as this | undefined;
  }

  parent(): LoroContainer | undefined {
    return this._parentLink?.container;
  }

  isDeleted(): boolean {
    return this._doc?._isContainerDeleted(this) ?? false;
  }

  subscribe(listener: (event: LoroEventBatch) => void): Subscription {
    return this._doc?._subscribeContainer(this, listener) ?? (() => {});
  }

  getShallowValue(): unknown {
    return this.toJSON();
  }

  _attach(doc: LoroDoc, id: CodecContainerId, parent: LoroContainer | undefined): void {
    this._doc = doc;
    this._codecId = id;
    this._containerId = formatContainerId(id);
    this._parentLink = parent === undefined ? undefined : { container: parent };
  }

  _setParentBinding(
    parent: LoroContainer,
    binding: NonNullable<ParentLink["binding"]>,
  ): void {
    this._parentLink = { container: parent, binding };
  }
}

export interface MapRecord {
  value: RuntimeValue | undefined;
  rawValue: RuntimeValue | undefined;
  deleted: boolean;
  writer: LastWriter;
}

interface MapKeyRecord {
  readonly key: string;
}

export class LoroMap<
  T extends Record<string, unknown> = Record<string, unknown>,
> extends LoroContainer {
  readonly _entries = new Map<string, MapRecord>();
  readonly _keyIndex = new OrderedIndex<MapKeyRecord>((left, right) =>
    left.key < right.key ? -1 : left.key > right.key ? 1 : 0,
  );
  readonly _keyRecords = new Map<string, MapKeyRecord>();

  kind(): "Map" {
    return "Map";
  }

  get<Key extends keyof T>(key: Key): T[Key] | undefined;
  get(key: string): unknown;
  get(key: string): unknown {
    const record = this._entries.get(key);
    return record === undefined || record.deleted
      ? undefined
      : cloneRuntimeValue(record.value!);
  }

  set<Key extends keyof T>(key: Key, value: T[Key]): void;
  set(key: string, value: unknown): void;
  set(key: string, value: unknown): void {
    if (isContainer(value)) {
      throw new TypeError("use setContainer() to attach a child container");
    }
    if (this._doc === undefined) {
      this._applyValue(key, normalizeDetachedValue(value), { peer: 0n, lamport: 0 });
      return;
    }
    this._doc._mapSet(this, key, value);
  }

  insert(key: string, value: unknown): void {
    this.set(key, value);
  }

  delete(key: string): void {
    if (this._doc === undefined) {
      if (this._entries.has(key)) this._removeVisibleKey(key);
      this._entries.delete(key);
      return;
    }
    this._doc._mapDelete(this, key);
  }

  clear(): void {
    for (const key of this.keys()) this.delete(key);
  }

  keys(): string[] {
    return this._keyIndex.values().map(({ key }) => key);
  }

  values(): unknown[] {
    const records = this._keyIndex.values();
    const output = new Array<unknown>(records.length);
    for (let index = 0; index < records.length; index += 1) {
      output[index] = this.get(records[index]!.key);
    }
    return output;
  }

  entries(): [string, unknown][] {
    const records = this._keyIndex.values();
    const output = new Array<[string, unknown]>(records.length);
    for (let index = 0; index < records.length; index += 1) {
      output[index] = [records[index]!.key, this.get(records[index]!.key)];
    }
    return output;
  }

  get size(): number {
    return this._keyIndex.size;
  }

  setContainer<C extends Container>(key: string, child: C): C {
    if (this._doc === undefined) {
      this._applyValue(key, child, { peer: 0n, lamport: 0 });
      return child;
    }
    return this._doc._mapSetContainer(this, key, child);
  }

  insertContainer<C extends Container>(key: string, child: C): C {
    return this.setContainer(key, child);
  }

  getOrCreateContainer<C extends Container>(key: string, child: C): C {
    const current = this.get(key);
    if (current === undefined || current === null) return this.setContainer(key, child);
    if (isContainer(current) && current.kind() === child.kind()) return current as C;
    throw new TypeError(`map key ${key} already contains an incompatible value`);
  }

  ensureMergeableMap(key: string): LoroMap {
    return this._ensureMergeable(key, "Map") as LoroMap;
  }

  ensureMergeableList(key: string): LoroList {
    return this._ensureMergeable(key, "List") as LoroList;
  }

  ensureMergeableMovableList(key: string): LoroMovableList {
    return this._ensureMergeable(key, "MovableList") as LoroMovableList;
  }

  ensureMergeableText(key: string): LoroText {
    return this._ensureMergeable(key, "Text") as LoroText;
  }

  ensureMergeableTree(key: string): LoroTree {
    return this._ensureMergeable(key, "Tree") as LoroTree;
  }

  ensureMergeableCounter(key: string): LoroCounter {
    return this._ensureMergeable(key, "Counter") as LoroCounter;
  }

  getLastEditor(key: string): string | undefined {
    return this._entries.get(key)?.writer.peer.toString();
  }

  toJSON(): Record<string, unknown> {
    const output: Record<string, unknown> = {};
    for (const { key } of this._keyIndex.values()) {
      output[key] = runtimeValueToJson(this._entries.get(key)!.value!);
    }
    return output;
  }

  override getShallowValue(): Record<string, unknown> {
    const output: Record<string, unknown> = {};
    for (const { key } of this._keyIndex.values()) {
      output[key] = runtimeValueToShallow(this._entries.get(key)!.value!);
    }
    return output;
  }

  _applyValue(
    key: string,
    value: RuntimeValue,
    writer: LastWriter,
    rawValue: RuntimeValue = value,
  ): void {
    const current = this._entries.get(key);
    if (current !== undefined && compareWriters(current.writer, writer) > 0) return;
    if (current === undefined || current.deleted) this._addVisibleKey(key);
    this._entries.set(key, { value, rawValue, deleted: false, writer });
    if (value instanceof LoroContainer) {
      value._setParentBinding(this, { kind: "map", key });
    }
  }

  _applyDelete(key: string, writer: LastWriter): void {
    const current = this._entries.get(key);
    if (current !== undefined && compareWriters(current.writer, writer) > 0) return;
    if (current !== undefined && !current.deleted) this._removeVisibleKey(key);
    this._entries.set(key, {
      value: undefined,
      rawValue: undefined,
      deleted: true,
      writer,
    });
  }

  _replaceRecord(key: string, record: MapRecord | undefined): void {
    const current = this._entries.get(key);
    if (current !== undefined && !current.deleted) this._removeVisibleKey(key);
    if (record === undefined) {
      this._entries.delete(key);
      return;
    }
    this._entries.set(key, record);
    if (!record.deleted) {
      this._addVisibleKey(key);
      if (record.value instanceof LoroContainer) {
        record.value._setParentBinding(this, { kind: "map", key });
      }
    }
  }

  _reset(): void {
    this._entries.clear();
    this._keyIndex.clear();
    this._keyRecords.clear();
  }

  private _addVisibleKey(key: string): void {
    let record = this._keyRecords.get(key);
    if (record === undefined) {
      record = { key };
      this._keyRecords.set(key, record);
    }
    this._keyIndex.add(record);
  }

  private _removeVisibleKey(key: string): void {
    const record = this._keyRecords.get(key);
    if (record !== undefined) this._keyIndex.delete(record);
  }

  private _ensureMergeable(key: string, type: ContainerType): Container {
    if (this._doc === undefined) {
      throw new Error("cannot ensure a mergeable child on a detached map");
    }
    return this._doc._mapEnsureMergeable(this, key, type);
  }
}

export class LoroList<T = unknown> extends LoroContainer {
  readonly _sequence = new SequenceIndex<SequenceElement>();
  _detachedCounter = 0;

  get _elements(): SequenceElement[] {
    return this._sequence.all();
  }

  kind(): ContainerType {
    return "List";
  }

  get length(): number {
    return this._sequence.visibleLength;
  }

  get(index: number): T | undefined {
    return cloneRuntimeValue(this._sequence.atVisible(index)?.value) as T | undefined;
  }

  toArray(): T[] {
    return this._visibleElements().map((element) =>
      cloneRuntimeValue(element.value),
    ) as T[];
  }

  insert(pos: number, value: T): void {
    this._validateInsertPosition(pos);
    if (isContainer(value))
      throw new TypeError("use insertContainer() to attach a child container");
    if (this._doc === undefined) {
      this._insertVisible(
        pos,
        [normalizeDetachedValue(value)],
        [this._nextDetachedId()],
        [0],
      );
      return;
    }
    this._doc._listInsert(this, pos, value);
  }

  push(value: T): void {
    this.insert(this.length, value);
  }

  delete(pos: number, len: number): void {
    validateRange(pos, len, this.length);
    if (len === 0) return;
    if (this._doc === undefined) {
      for (const element of this._sequence.visibleRange(pos, pos + len))
        this._sequence.setDeleted(element, true);
      return;
    }
    this._doc._sequenceDelete(this, pos, len);
  }

  clear(): void {
    this.delete(0, this.length);
  }

  pop(): T | undefined {
    if (this.length === 0) return undefined;
    const value = this.get(this.length - 1);
    this.delete(this.length - 1, 1);
    return value;
  }

  insertContainer<C extends Container>(pos: number, child: C): C {
    this._validateInsertPosition(pos);
    if (this._doc === undefined) {
      this._insertVisible(pos, [child], [this._nextDetachedId()], [0]);
      return child;
    }
    return this._doc._listInsertContainer(this, pos, child);
  }

  pushContainer<C extends Container>(child: C): C {
    return this.insertContainer(this.length, child);
  }

  getIdAt(pos: number): { peer: string; counter: number } | undefined {
    const id = this._sequence.atVisible(pos)?.id;
    return id === undefined
      ? undefined
      : { peer: id.peer.toString(), counter: id.counter };
  }

  getCursor(pos: number, side: Side = 0): Cursor | undefined {
    if (!Number.isSafeInteger(pos) || pos < 0) return undefined;
    if (pos >= this.length) {
      return new Cursor(
        this.id,
        this._sequence.atVisible(this.length - 1)?.id,
        1,
        this.length,
      );
    }
    return new Cursor(this.id, this._sequence.atVisible(pos)!.id, side, pos);
  }

  toJSON(): unknown[] {
    return this._visibleElements().map((element) => runtimeValueToJson(element.value));
  }

  override getShallowValue(): unknown[] {
    return this._visibleElements().map((element) => runtimeValueToShallow(element.value));
  }

  _visibleElements(): SequenceElement[] {
    return this._sequence.visible();
  }

  _visibleElementsRange(start: number, end: number): SequenceElement[] {
    return this._sequence.visibleRange(start, end);
  }

  _valuesRange(start: number, end: number): unknown[] {
    return this._sequence
      .visibleRange(start, end)
      .map((element) => cloneRuntimeValue(element.value));
  }

  _visibleElementAt(position: number): SequenceElement | undefined {
    return this._sequence.atVisible(position);
  }

  _insertVisible(
    position: number,
    values: readonly RuntimeValue[],
    ids: readonly CodecId[],
    lamports: readonly number[],
  ): void {
    const movable = this instanceof LoroMovableList;
    const elements: SequenceElement[] = values.map((value, index) => {
      const id = ids[index]!;
      const lamport = lamports[index]!;
      return {
        value,
        id,
        lamport,
        deleted: false,
        originLeft: undefined,
        originRight: undefined,
        valueHistory: movable ? [{ id, lamport, value }] : undefined,
      };
    });
    this._sequence.insertAtVisible(position, elements);
    this._bindChildren(elements);
  }

  _insertFugue(
    position: number,
    values: readonly RuntimeValue[],
    ids: readonly CodecId[],
    lamports: readonly number[],
    causalVersion: CausalVersion,
  ): void {
    const movable = this instanceof LoroMovableList;
    const elements: SequenceElement[] = values.map((value, index) => {
      const id = ids[index]!;
      const elementLamport = lamports[index]!;
      return {
        value,
        id,
        lamport: elementLamport,
        deleted: false,
        originLeft: undefined,
        originRight: undefined,
        valueHistory: movable ? [{ id, lamport: elementLamport, value }] : undefined,
      };
    });
    insertFugueElements(
      this._sequence,
      position,
      elements,
      causalVersion,
      // Moves break the origin-tree preorder used by the direct-child index.
      !(this instanceof LoroMovableList),
    );
    this._bindChildren(elements);
  }

  _deleteIdSpan(startId: CodecId, length: number, deletedBy?: CodecId): void {
    if (Math.abs(length) === 1) {
      const element = this._sequence.findById(startId);
      if (element === undefined) return;
      this._sequence.deleteElement(element, deletedBy);
      return;
    }
    this._sequence.deleteIdSpan(startId, length, deletedBy);
  }

  _validateInsertPosition(position: number): void {
    if (!Number.isSafeInteger(position) || position < 0 || position > this.length) {
      throw new RangeError(`list position ${position} is out of range`);
    }
  }

  _nextDetachedId(): CodecId {
    return { peer: 0n, counter: this._detachedCounter++ };
  }

  _reset(): void {
    this._sequence.reset();
    this._detachedCounter = 0;
  }

  _bindChildren(elements: readonly SequenceElement[]): void {
    for (const element of elements) {
      if (element.value instanceof LoroContainer) {
        element.value._setParentBinding(this, { kind: "sequence", element });
      }
    }
  }
}

export interface TextElement extends SequenceElement {
  value: string;
  attributes?: Map<string, RuntimeValue> | undefined;
  attributeMeta?: Map<string, TextStyleMeta> | undefined;
  attributeHistory?: Map<string, TextStyleMeta[]> | undefined;
}

const TEXT_NUMBER_STRIDE = 6;
const TEXT_PEER_STRIDE = 4;
const TEXT_COUNTER = 0;
const TEXT_LAMPORT = 1;
const TEXT_UTF16_END = 2;
const TEXT_ORIGIN_LEFT_COUNTER = 3;
const TEXT_ORIGIN_RIGHT_COUNTER = 4;
const TEXT_DELETED_BY_COUNTER = 5;
const TEXT_ID_PEER = 0;
const TEXT_ORIGIN_LEFT_PEER = 1;
const TEXT_ORIGIN_RIGHT_PEER = 2;
const TEXT_DELETED_BY_PEER = 3;
const NO_DELETION_IDS: readonly CodecId[] = [];

class TextSequenceSpan extends SequenceSpan<TextElement> {
  #text = "";
  #length = 0;
  #numbers: number[] | undefined;
  #peers: (bigint | undefined)[] | undefined;
  #pendingStartPeer: bigint | undefined;
  #pendingStartCounter = 0;
  #pendingLamport = 0;
  #pendingOriginLeftPeer: bigint | undefined;
  #pendingOriginLeftCounter = 0;
  #pendingOriginRightPeer: bigint | undefined;
  #pendingOriginRightCounter = 0;
  #deletedBits = 0;
  #retained: Map<number, PackedTextElement> | undefined;
  #deletedBy: Map<number, CodecId[]> | undefined;
  #attributes: Map<number, Map<string, RuntimeValue>> | undefined;
  #attributeMeta: Map<number, Map<string, TextStyleMeta>> | undefined;
  #attributeHistory: Map<number, Map<string, TextStyleMeta[]>> | undefined;
  #valueHistory: Map<number, SequenceValueMeta[]> | undefined;
  #moveHistory: Map<number, SequenceMoveMeta[]> | undefined;

  static fromText(
    text: string,
    startId: CodecId,
    lamport: number,
    originLeft: CodecId | undefined,
    originRight: CodecId | undefined,
  ): TextSequenceSpan {
    return TextSequenceSpan.fromTextOrigins(
      text,
      startId,
      lamport,
      originLeft?.peer,
      originLeft?.counter ?? 0,
      originRight?.peer,
      originRight?.counter ?? 0,
    );
  }

  static fromTextOrigins(
    text: string,
    startId: CodecId,
    lamport: number,
    originLeftPeer: bigint | undefined,
    originLeftCounter: number,
    originRightPeer: bigint | undefined,
    originRightCounter: number,
  ): TextSequenceSpan {
    const span = new TextSequenceSpan();
    span.#text = text;
    for (const _value of text) span.#length += 1;
    span.#pendingStartPeer = startId.peer;
    span.#pendingStartCounter = startId.counter;
    span.#pendingLamport = lamport;
    span.#pendingOriginLeftPeer = originLeftPeer;
    span.#pendingOriginLeftCounter = originLeftCounter;
    span.#pendingOriginRightPeer = originRightPeer;
    span.#pendingOriginRightCounter = originRightCounter;
    return span;
  }

  /** The caller must have validated ID uniqueness across the complete snapshot. */
  static fromValidatedSnapshotChunk(
    text: string,
    peers: readonly bigint[],
    counters: readonly number[],
    lamports: readonly number[],
  ): TextSequenceSpan {
    if (
      peers.length === 0 ||
      peers.length !== counters.length ||
      peers.length !== lamports.length
    ) {
      throw new Error("invalid text snapshot chunk columns");
    }
    const span = new TextSequenceSpan();
    span.#text = text;
    span.#length = peers.length;
    const numbers = new Array<number>(peers.length * TEXT_NUMBER_STRIDE).fill(0);
    const peerColumns = new Array<bigint | undefined>(
      peers.length * TEXT_PEER_STRIDE,
    ).fill(undefined);
    let utf16End = 0;
    let offset = 0;
    for (const value of text) {
      if (offset >= peers.length) {
        throw new Error("text snapshot chunk contains too many Unicode scalars");
      }
      utf16End += value.length;
      const numberBase = offset * TEXT_NUMBER_STRIDE;
      numbers[numberBase + TEXT_COUNTER] = counters[offset]!;
      numbers[numberBase + TEXT_LAMPORT] = lamports[offset]!;
      numbers[numberBase + TEXT_UTF16_END] = utf16End;
      const peerBase = offset * TEXT_PEER_STRIDE;
      peerColumns[peerBase + TEXT_ID_PEER] = peers[offset]!;
      offset += 1;
    }
    if (offset !== peers.length) {
      throw new Error("text snapshot chunk contains too few Unicode scalars");
    }
    span.#numbers = numbers;
    span.#peers = peerColumns;
    return span;
  }

  get length(): number {
    return this.#length;
  }

  override idsAreUnique(): boolean {
    return true;
  }

  elementAt(offset: number): TextElement {
    return this.#retained?.get(offset) ?? new PackedTextElement(this, offset);
  }

  idAt(offset: number): CodecId {
    return {
      peer: this.peerAt(offset),
      counter: this.counterAt(offset),
    };
  }

  override peerAt(offset: number): bigint {
    return this.#peer(offset, TEXT_ID_PEER)!;
  }

  override counterAt(offset: number): number {
    return this.#number(offset, TEXT_COUNTER);
  }

  lamportAt(offset: number): number {
    return this.#number(offset, TEXT_LAMPORT);
  }

  deletedAt(offset: number): boolean {
    return ((this.#deletedBits >>> offset) & 1) !== 0;
  }

  setDeletedAt(offset: number, deleted: boolean): void {
    const bit = 1 << offset;
    this.#deletedBits = deleted
      ? (this.#deletedBits | bit) >>> 0
      : (this.#deletedBits & ~bit) >>> 0;
  }

  metricsAt(offset: number): { utf16: number; utf8: number } {
    const value = this.valueAt(offset);
    return { utf16: value.length, utf8: utf8CodePointLength(value) };
  }

  slice(start: number, end: number): TextSequenceSpan {
    this.#materializeColumns();
    const output = new TextSequenceSpan();
    output.#numbers = [];
    output.#peers = [];
    output.#length = end - start;
    const utf16Start = start === 0 ? 0 : this.#number(start - 1, TEXT_UTF16_END);
    const utf16End = end === 0 ? 0 : this.#number(end - 1, TEXT_UTF16_END);
    output.#text = this.#text.slice(utf16Start, utf16End);
    for (let offset = start; offset < end; offset += 1) {
      const numberBase = offset * TEXT_NUMBER_STRIDE;
      for (let column = 0; column < TEXT_NUMBER_STRIDE; column += 1) {
        const value = this.#numbers![numberBase + column]!;
        output.#numbers.push(column === TEXT_UTF16_END ? value - utf16Start : value);
      }
      const peerBase = offset * TEXT_PEER_STRIDE;
      output.#peers.push(
        this.#peers![peerBase],
        this.#peers![peerBase + 1],
        this.#peers![peerBase + 2],
        this.#peers![peerBase + 3],
      );
      const retained = this.#retained?.get(offset);
      if (retained !== undefined) {
        const nextOffset = offset - start;
        retained._retarget(output, nextOffset);
        (output.#retained ??= new Map()).set(nextOffset, retained);
      }
    }
    const length = end - start;
    const mask = length === 32 ? 0xffff_ffff : (1 << length) - 1;
    output.#deletedBits = (this.#deletedBits >>> start) & mask;
    output.#deletedBy = copyOffsetMap(this.#deletedBy, start, end);
    output.#attributes = copyOffsetMap(this.#attributes, start, end);
    output.#attributeMeta = copyOffsetMap(this.#attributeMeta, start, end);
    output.#attributeHistory = copyOffsetMap(this.#attributeHistory, start, end);
    output.#valueHistory = copyOffsetMap(this.#valueHistory, start, end);
    output.#moveHistory = copyOffsetMap(this.#moveHistory, start, end);
    return output;
  }

  override append(other: SequenceSpan<TextElement>): boolean {
    if (!(other instanceof TextSequenceSpan)) return false;
    const oldLength = this.length;
    const utf16Base = this.#text.length;
    this.#materializeColumns();
    this.#text += other.#text;
    for (let offset = 0; offset < other.length; offset += 1) {
      for (let column = 0; column < TEXT_NUMBER_STRIDE; column += 1) {
        const value = other.#number(offset, column);
        this.#numbers!.push(column === TEXT_UTF16_END ? value + utf16Base : value);
      }
      this.#peers!.push(
        other.#peer(offset, 0),
        other.#peer(offset, 1),
        other.#peer(offset, 2),
        other.#peer(offset, 3),
      );
      const retained = other.#retained?.get(offset);
      if (retained !== undefined) {
        const nextOffset = oldLength + offset;
        retained._retarget(this, nextOffset);
        (this.#retained ??= new Map()).set(nextOffset, retained);
      }
    }
    this.#deletedBits = (this.#deletedBits | (other.#deletedBits << oldLength)) >>> 0;
    this.#length += other.length;
    this.#deletedBy = appendOffsetMap(this.#deletedBy, other.#deletedBy, oldLength);
    this.#attributes = appendOffsetMap(this.#attributes, other.#attributes, oldLength);
    this.#attributeMeta = appendOffsetMap(
      this.#attributeMeta,
      other.#attributeMeta,
      oldLength,
    );
    this.#attributeHistory = appendOffsetMap(
      this.#attributeHistory,
      other.#attributeHistory,
      oldLength,
    );
    this.#valueHistory = appendOffsetMap(
      this.#valueHistory,
      other.#valueHistory,
      oldLength,
    );
    this.#moveHistory = appendOffsetMap(this.#moveHistory, other.#moveHistory, oldLength);
    return true;
  }

  override deletionIdsAt(offset: number): readonly CodecId[] {
    const deletedBy = this.#deletedBy?.get(offset);
    const peer = this.#peer(offset, TEXT_DELETED_BY_PEER);
    if (peer === undefined) return deletedBy ?? NO_DELETION_IDS;
    const length = deletedBy?.length ?? 0;
    const output = new Array<CodecId>(length + 1);
    output[0] = {
      peer,
      counter: this.#number(offset, TEXT_DELETED_BY_COUNTER),
    };
    for (let index = 0; index < length; index += 1) {
      output[index + 1] = deletedBy![index]!;
    }
    return output;
  }

  override retain(offset: number, element: TextElement): void {
    if (element instanceof PackedTextElement) {
      (this.#retained ??= new Map()).set(offset, element);
    }
  }

  override forEachRetained(visit: (element: TextElement, offset: number) => void): void {
    for (const [offset, element] of this.#retained ?? []) visit(element, offset);
  }

  valueAt(offset: number): string {
    const start = offset === 0 ? 0 : this.#number(offset - 1, TEXT_UTF16_END);
    return this.#text.slice(start, this.#number(offset, TEXT_UTF16_END));
  }

  textRange(start: number, end: number): string {
    const utf16Start = start === 0 ? 0 : this.#number(start - 1, TEXT_UTF16_END);
    const utf16End = end === 0 ? 0 : this.#number(end - 1, TEXT_UTF16_END);
    return this.#text.slice(utf16Start, utf16End);
  }

  setValueAt(offset: number, value: string): void {
    const current = this.valueAt(offset);
    if (current === value) return;
    this.#materializeColumns();
    const start = offset === 0 ? 0 : this.#number(offset - 1, TEXT_UTF16_END);
    const end = this.#number(offset, TEXT_UTF16_END);
    this.#text = this.#text.slice(0, start) + value + this.#text.slice(end);
    const difference = value.length - (end - start);
    for (let index = offset; index < this.length; index += 1) {
      this.#setNumber(
        index,
        TEXT_UTF16_END,
        this.#number(index, TEXT_UTF16_END) + difference,
      );
    }
  }

  originLeftAt(offset: number): CodecId | undefined {
    return this.#idColumnAt(offset, TEXT_ORIGIN_LEFT_PEER, TEXT_ORIGIN_LEFT_COUNTER);
  }

  setOriginLeftAt(offset: number, id: CodecId | undefined): void {
    this.#setIdColumn(offset, TEXT_ORIGIN_LEFT_PEER, TEXT_ORIGIN_LEFT_COUNTER, id);
  }

  originRightAt(offset: number): CodecId | undefined {
    return this.#idColumnAt(offset, TEXT_ORIGIN_RIGHT_PEER, TEXT_ORIGIN_RIGHT_COUNTER);
  }

  setOriginRightAt(offset: number, id: CodecId | undefined): void {
    this.#setIdColumn(offset, TEXT_ORIGIN_RIGHT_PEER, TEXT_ORIGIN_RIGHT_COUNTER, id);
  }

  deletedByPeerAt(offset: number): bigint | undefined {
    return this.#peer(offset, TEXT_DELETED_BY_PEER);
  }

  setDeletedByPeerAt(offset: number, peer: bigint | undefined): void {
    this.#setPeer(offset, TEXT_DELETED_BY_PEER, peer);
  }

  deletedByCounterAt(offset: number): number | undefined {
    return this.#peer(offset, TEXT_DELETED_BY_PEER) === undefined
      ? undefined
      : this.#number(offset, TEXT_DELETED_BY_COUNTER);
  }

  setDeletedByCounterAt(offset: number, counter: number | undefined): void {
    this.#setNumber(offset, TEXT_DELETED_BY_COUNTER, counter ?? 0);
  }

  deletedByAt(offset: number): CodecId[] | undefined {
    return this.#deletedBy?.get(offset);
  }

  setDeletedByAt(offset: number, ids: CodecId[] | undefined): void {
    this.#deletedBy = setOffsetMap(this.#deletedBy, offset, ids);
  }

  attributesAt(offset: number): Map<string, RuntimeValue> | undefined {
    return this.#attributes?.get(offset);
  }

  setAttributesAt(offset: number, value: Map<string, RuntimeValue> | undefined): void {
    this.#attributes = setOffsetMap(this.#attributes, offset, value);
  }

  attributeMetaAt(offset: number): Map<string, TextStyleMeta> | undefined {
    return this.#attributeMeta?.get(offset);
  }

  setAttributeMetaAt(
    offset: number,
    value: Map<string, TextStyleMeta> | undefined,
  ): void {
    this.#attributeMeta = setOffsetMap(this.#attributeMeta, offset, value);
  }

  attributeHistoryAt(offset: number): Map<string, TextStyleMeta[]> | undefined {
    return this.#attributeHistory?.get(offset);
  }

  setAttributeHistoryAt(
    offset: number,
    value: Map<string, TextStyleMeta[]> | undefined,
  ): void {
    this.#attributeHistory = setOffsetMap(this.#attributeHistory, offset, value);
  }

  valueHistoryAt(offset: number): SequenceValueMeta[] | undefined {
    return this.#valueHistory?.get(offset);
  }

  setValueHistoryAt(offset: number, value: SequenceValueMeta[] | undefined): void {
    this.#valueHistory = setOffsetMap(this.#valueHistory, offset, value);
  }

  moveHistoryAt(offset: number): SequenceMoveMeta[] | undefined {
    return this.#moveHistory?.get(offset);
  }

  setMoveHistoryAt(offset: number, value: SequenceMoveMeta[] | undefined): void {
    this.#moveHistory = setOffsetMap(this.#moveHistory, offset, value);
  }

  #number(offset: number, column: number): number {
    if (this.#numbers !== undefined) {
      return this.#numbers[offset * TEXT_NUMBER_STRIDE + column]!;
    }
    switch (column) {
      case TEXT_COUNTER:
        return this.#pendingStartCounter + offset;
      case TEXT_LAMPORT:
        return this.#pendingLamport + offset;
      case TEXT_UTF16_END: {
        let utf16End = 0;
        let index = 0;
        for (const value of this.#text) {
          utf16End += value.length;
          if (index === offset) return utf16End;
          index += 1;
        }
        throw new RangeError("text span offset is out of range");
      }
      case TEXT_ORIGIN_LEFT_COUNTER:
        return offset === 0
          ? this.#pendingOriginLeftCounter
          : this.#pendingStartCounter + offset - 1;
      case TEXT_ORIGIN_RIGHT_COUNTER:
        return this.#pendingOriginRightCounter;
      case TEXT_DELETED_BY_COUNTER:
        return 0;
      default:
        throw new RangeError("text span number column is out of range");
    }
  }

  #setNumber(offset: number, column: number, value: number): void {
    this.#materializeColumns();
    this.#numbers![offset * TEXT_NUMBER_STRIDE + column] = value;
  }

  #peer(offset: number, column: number): bigint | undefined {
    if (this.#peers !== undefined) {
      return this.#peers[offset * TEXT_PEER_STRIDE + column];
    }
    switch (column) {
      case TEXT_ID_PEER:
        return this.#pendingStartPeer;
      case TEXT_ORIGIN_LEFT_PEER:
        return offset === 0 ? this.#pendingOriginLeftPeer : this.#pendingStartPeer;
      case TEXT_ORIGIN_RIGHT_PEER:
        return this.#pendingOriginRightPeer;
      case TEXT_DELETED_BY_PEER:
        return undefined;
      default:
        throw new RangeError("text span peer column is out of range");
    }
  }

  #setPeer(offset: number, column: number, value: bigint | undefined): void {
    this.#materializeColumns();
    this.#peers![offset * TEXT_PEER_STRIDE + column] = value;
  }

  #materializeColumns(): void {
    if (this.#numbers !== undefined) return;
    const numbers: number[] = [];
    const peers: (bigint | undefined)[] = [];
    let utf16End = 0;
    let offset = 0;
    for (const value of this.#text) {
      utf16End += value.length;
      numbers.push(
        this.#pendingStartCounter + offset,
        this.#pendingLamport + offset,
        utf16End,
        offset === 0
          ? this.#pendingOriginLeftCounter
          : this.#pendingStartCounter + offset - 1,
        this.#pendingOriginRightCounter,
        0,
      );
      peers.push(
        this.#pendingStartPeer,
        offset === 0 ? this.#pendingOriginLeftPeer : this.#pendingStartPeer,
        this.#pendingOriginRightPeer,
        undefined,
      );
      offset += 1;
    }
    this.#numbers = numbers;
    this.#peers = peers;
  }

  #idColumnAt(
    offset: number,
    peerColumn: number,
    counterColumn: number,
  ): CodecId | undefined {
    const peer = this.#peer(offset, peerColumn);
    return peer === undefined
      ? undefined
      : { peer, counter: this.#number(offset, counterColumn) };
  }

  #setIdColumn(
    offset: number,
    peerColumn: number,
    counterColumn: number,
    id: CodecId | undefined,
  ): void {
    this.#setPeer(offset, peerColumn, id?.peer);
    this.#setNumber(offset, counterColumn, id?.counter ?? 0);
  }
}

class PackedTextElement implements TextElement, CodecId {
  #span: TextSequenceSpan;
  #offset: number;
  readonly peer: bigint;
  readonly counter: number;

  constructor(span: TextSequenceSpan, offset: number) {
    this.#span = span;
    this.#offset = offset;
    const id = span.idAt(offset);
    this.peer = id.peer;
    this.counter = id.counter;
  }

  get id(): CodecId {
    return this;
  }

  get lamport(): number {
    return this.#span.lamportAt(this.#offset);
  }

  set lamport(_value: number) {
    throw new TypeError("text element lamport is immutable");
  }

  get value(): string {
    return this.#span.valueAt(this.#offset);
  }

  set value(value: string) {
    this.#span.setValueAt(this.#offset, value);
  }

  get deleted(): boolean {
    return this.#span.deletedAt(this.#offset);
  }

  set deleted(value: boolean) {
    this.#span.setDeletedAt(this.#offset, value);
  }

  get originLeft(): CodecId | undefined {
    return this.#span.originLeftAt(this.#offset);
  }

  set originLeft(value: CodecId | undefined) {
    this.#span.setOriginLeftAt(this.#offset, value);
  }

  get originRight(): CodecId | undefined {
    return this.#span.originRightAt(this.#offset);
  }

  set originRight(value: CodecId | undefined) {
    this.#span.setOriginRightAt(this.#offset, value);
  }

  get deletedByPeer(): bigint | undefined {
    return this.#span.deletedByPeerAt(this.#offset);
  }

  set deletedByPeer(value: bigint | undefined) {
    this.#span.setDeletedByPeerAt(this.#offset, value);
  }

  get deletedByCounter(): number | undefined {
    return this.#span.deletedByCounterAt(this.#offset);
  }

  set deletedByCounter(value: number | undefined) {
    this.#span.setDeletedByCounterAt(this.#offset, value);
  }

  get deletedBy(): CodecId[] | undefined {
    return this.#span.deletedByAt(this.#offset);
  }

  set deletedBy(value: CodecId[] | undefined) {
    this.#span.setDeletedByAt(this.#offset, value);
  }

  get attributes(): Map<string, RuntimeValue> | undefined {
    return this.#span.attributesAt(this.#offset);
  }

  set attributes(value: Map<string, RuntimeValue> | undefined) {
    this.#span.setAttributesAt(this.#offset, value);
  }

  get attributeMeta(): Map<string, TextStyleMeta> | undefined {
    return this.#span.attributeMetaAt(this.#offset);
  }

  set attributeMeta(value: Map<string, TextStyleMeta> | undefined) {
    this.#span.setAttributeMetaAt(this.#offset, value);
  }

  get attributeHistory(): Map<string, TextStyleMeta[]> | undefined {
    return this.#span.attributeHistoryAt(this.#offset);
  }

  set attributeHistory(value: Map<string, TextStyleMeta[]> | undefined) {
    this.#span.setAttributeHistoryAt(this.#offset, value);
  }

  get valueHistory(): SequenceValueMeta[] | undefined {
    return this.#span.valueHistoryAt(this.#offset);
  }

  set valueHistory(value: SequenceValueMeta[] | undefined) {
    this.#span.setValueHistoryAt(this.#offset, value);
  }

  get moveHistory(): SequenceMoveMeta[] | undefined {
    return this.#span.moveHistoryAt(this.#offset);
  }

  set moveHistory(value: SequenceMoveMeta[] | undefined) {
    this.#span.setMoveHistoryAt(this.#offset, value);
  }

  _retarget(span: TextSequenceSpan, offset: number): void {
    this.#span = span;
    this.#offset = offset;
  }
}

function setOffsetMap<T>(
  map: Map<number, T> | undefined,
  offset: number,
  value: T | undefined,
): Map<number, T> | undefined {
  if (value === undefined) {
    map?.delete(offset);
    return map;
  }
  const output = map ?? new Map<number, T>();
  output.set(offset, value);
  return output;
}

function copyOffsetMap<T>(
  map: ReadonlyMap<number, T> | undefined,
  start: number,
  end: number,
): Map<number, T> | undefined {
  if (map === undefined) return undefined;
  const output = new Map<number, T>();
  for (const [offset, value] of map) {
    if (offset >= start && offset < end) output.set(offset - start, value);
  }
  return output.size === 0 ? undefined : output;
}

function appendOffsetMap<T>(
  left: Map<number, T> | undefined,
  right: ReadonlyMap<number, T> | undefined,
  offset: number,
): Map<number, T> | undefined {
  if (right === undefined) return left;
  const output = left ?? new Map<number, T>();
  for (const [index, value] of right) output.set(offset + index, value);
  return output;
}

/** Reads a visible range from sequence storage without materializing scalar views. */
function storageTextRange(
  storage: SequenceStorage<TextElement>,
  start: number,
  end: number,
): string {
  if (storage instanceof TextSequenceSpan) return storage.textRange(start, end);
  if (storage instanceof SequenceSpan) {
    throw new Error("unexpected text sequence span implementation");
  }
  if (Array.isArray(storage)) {
    const elements = storage as readonly TextElement[];
    let text = "";
    for (let offset = start; offset < end; offset += 1) text += elements[offset]!.value;
    return text;
  }
  return (storage as TextElement).value;
}

export interface TextStyleMeta {
  readonly startId: CodecId;
  readonly lamport: number;
  readonly info: number;
  readonly value: RuntimeValue;
}

export class LoroText extends LoroContainer {
  readonly _sequence = new SequenceIndex<TextElement>((element) => ({
    utf16: element.value.length,
    utf8: utf8CodePointLength(element.value),
  }));
  _detachedCounter = 0;
  _detachedStyleCounter = 0;
  _attributeHistoryComplete = true;
  readonly _styleIndex = new TextStyleIndex<TextStyleMeta>();
  _styleVersion: CausalVersion | undefined;
  readonly #insertionIdContext: SequenceInsertionIdContext = {
    leftPeer: undefined,
    leftCounter: 0,
    startIndex: 0,
    rightPeer: undefined,
    rightCounter: 0,
  };
  #validatedSnapshotSpans: TextSequenceSpan[] | undefined;
  readonly #attributeValuesCache = new WeakMap<
    ReadonlyMap<string, TextStyleMeta>,
    ReadonlyMap<string, RuntimeValue>
  >();

  get _elements(): TextElement[] {
    return this._sequence.all();
  }

  kind(): "Text" {
    return "Text";
  }

  get length(): number {
    return this._sequence.visibleUtf16Length;
  }

  toString(): string {
    return this._stringRange(0, this._sequence.visibleLength);
  }

  _stringRange(start: number, end: number): string {
    const chunks: string[] = [];
    let cursor = 0;
    this._sequence.forEachVisibleStorageRange((storage, rangeStart, rangeEnd) => {
      const nextCursor = cursor + (rangeEnd - rangeStart);
      if (nextCursor > start && end > cursor) {
        chunks.push(
          storageTextRange(
            storage,
            start > cursor ? rangeStart + (start - cursor) : rangeStart,
            end < nextCursor ? rangeStart + (end - cursor) : rangeEnd,
          ),
        );
      }
      cursor = nextCursor;
      return nextCursor >= end ? false : undefined;
    });
    return chunks.join("");
  }

  iter(callback: (chunk: string) => boolean | void | null): void {
    this._sequence.forEachVisibleStorageRange((storage, start, end) =>
      callback(storageTextRange(storage, start, end)) === false ? false : undefined,
    );
  }

  insert(pos: number, text: string): void {
    const unicodePosition = this._validateInsertPosition(pos);
    if (text.length === 0) return;
    if (this._doc === undefined) {
      const span = TextSequenceSpan.fromText(
        text,
        { peer: 0n, counter: this._detachedCounter },
        0,
        undefined,
        undefined,
      );
      this._detachedCounter += span.length;
      this._sequence.insertSpanAtVisible(unicodePosition, span);
      return;
    }
    this._doc._textInsert(this, unicodePosition, text);
  }

  push(text: string): void {
    this.insert(this.length, text);
  }

  insertUtf8(index: number, text: string): void {
    const utf16 = this.convertPos(index, "utf8", "utf16");
    if (utf16 === undefined) throw new RangeError("UTF-8 position is out of range");
    this.insert(utf16, text);
  }

  delete(pos: number, len: number): void {
    validateRange(pos, len, this.length);
    if (len === 0) return;
    const start = this._unicodePosition(pos);
    const end = this._unicodePosition(pos + len);
    if (this._doc === undefined) {
      for (const element of this._sequence.visibleRange(start, end))
        this._sequence.setDeleted(element, true);
      return;
    }
    this._doc._textDelete(this, start, end - start);
  }

  deleteUtf8(index: number, length: number): void {
    const start = this.convertPos(index, "utf8", "utf16");
    const end = this.convertPos(index + length, "utf8", "utf16");
    if (start === undefined || end === undefined)
      throw new RangeError("UTF-8 range is out of bounds");
    this.delete(start, end - start);
  }

  slice(start: number, end: number): string {
    validateRange(start, end - start, this.length);
    const unicodeStart = this._unicodePosition(start);
    const unicodeEnd = this._unicodePosition(end);
    return this._stringRange(unicodeStart, unicodeEnd);
  }

  charAt(pos: number): string {
    if (pos === this.length) return "";
    return this._sequence.atVisible(this._unicodePosition(pos))?.value ?? "";
  }

  splice(pos: number, len: number, text: string): string {
    const removed = this.slice(pos, pos + len);
    this.delete(pos, len);
    this.insert(pos, text);
    return removed;
  }

  mark(range: { start: number; end: number }, key: string, value: unknown): void {
    validateRange(range.start, range.end - range.start, this.length);
    const start = this._unicodePosition(range.start);
    const end = this._unicodePosition(range.end);
    if (this._doc === undefined) {
      this._applyMark(start, end, key, normalizeDetachedValue(value));
      return;
    }
    this._doc._textMark(this, start, end, key, value);
  }

  unmark(range: { start: number; end: number }, key: string): void {
    validateRange(range.start, range.end - range.start, this.length);
    const start = this._unicodePosition(range.start);
    const end = this._unicodePosition(range.end);
    if (
      !this._styleIndex.rangeHasKey(this._styleRuns(start, end), key, this._styleVersion)
    ) {
      return;
    }
    this.mark(range, key, null);
  }

  applyDelta(delta: readonly Delta<string>[]): void {
    let position = 0;
    const marks: {
      readonly start: number;
      readonly end: number;
      readonly attributes: Readonly<Record<string, Value>>;
    }[] = [];
    for (const operation of delta) {
      if ("insert" in operation) {
        this.insert(position, operation.insert);
        const length = operation.insert.length;
        if (operation.attributes !== undefined) {
          marks.push({
            start: position,
            end: position + length,
            attributes: operation.attributes,
          });
        }
        position += length;
      } else if ("delete" in operation) {
        this.delete(position, operation.delete);
      } else {
        if (operation.attributes !== undefined) {
          marks.push({
            start: position,
            end: position + operation.retain,
            attributes: operation.attributes,
          });
        }
        position += operation.retain;
      }
    }
    for (const { start, end, attributes } of marks) {
      for (const [key, value] of Object.entries(attributes)) {
        this.mark({ start, end }, key, value);
      }
    }
  }

  toDelta(): Delta<string>[] {
    return textElementsToDelta(this._visibleElements(), this.#attributeResolver());
  }

  sliceDelta(start: number, end: number): Delta<string>[] {
    validateRange(start, end - start, this.length);
    const unicodeStart = this._unicodePosition(start);
    const unicodeEnd = this._unicodePosition(end);
    return textElementsToDelta(
      this._sequence.visibleRange(unicodeStart, unicodeEnd),
      this.#attributeResolver(),
    );
  }

  sliceDeltaUtf8(start: number, end: number): Delta<string>[] {
    const utf16Start = this.convertPos(start, "utf8", "utf16");
    const utf16End = this.convertPos(end, "utf8", "utf16");
    if (utf16Start === undefined || utf16End === undefined) {
      throw new RangeError("UTF-8 range is out of bounds");
    }
    return this.sliceDelta(utf16Start, utf16End);
  }

  convertPos(index: number, from: TextPosType, to: TextPosType): number | undefined {
    if (!isTextPosType(from) || !isTextPosType(to)) return undefined;
    const visibleLength = this._sequence.visibleLength;
    const directMetric =
      from === "unicode" ||
      (from === "utf16" && this._sequence.visibleUtf16Length === visibleLength) ||
      (from === "utf8" && this._sequence.visibleUtf8Length === visibleLength);
    const unicodeIndex = directMetric
      ? Number.isSafeInteger(index) && index >= 0 && index <= visibleLength
        ? index
        : undefined
      : this._sequence.visibleIndexAtMetricOffset(index, from);
    if (unicodeIndex === undefined) return undefined;
    if (
      to === "unicode" ||
      (to === "utf16" && this._sequence.visibleUtf16Length === visibleLength) ||
      (to === "utf8" && this._sequence.visibleUtf8Length === visibleLength)
    ) {
      return unicodeIndex;
    }
    return this._sequence.metricOffsetAtVisibleIndex(unicodeIndex, to);
  }

  getCursor(pos: number, side: Side = 0): Cursor | undefined {
    if (!Number.isSafeInteger(pos) || pos < 0) return undefined;
    if (pos >= this.length) {
      return new Cursor(
        this.id,
        this._sequence.atVisible(this._sequence.visibleLength - 1)?.id,
        1,
        this.length,
      );
    }
    const unicodePosition = this.convertPos(pos, "utf16", "unicode");
    if (unicodePosition === undefined) return undefined;
    return new Cursor(this.id, this._sequence.atVisible(unicodePosition)!.id, side, pos);
  }

  getEditorOf(pos: number): string | undefined {
    const unicodePosition = this.convertPos(pos, "utf16", "unicode");
    return unicodePosition === undefined
      ? undefined
      : this._sequence.atVisible(unicodePosition)?.id.peer.toString();
  }

  update(text: string, _options?: TextUpdateOptions): void {
    updateText(this, text);
  }

  updateByLine(text: string, options?: TextUpdateOptions): void {
    this.update(text, options);
  }

  toJSON(): string {
    return this.toString();
  }

  override getShallowValue(): string {
    return this.toString();
  }

  _visibleElements(): TextElement[] {
    return this._sequence.visible();
  }

  _visibleElementsRange(start: number, end: number): TextElement[] {
    return this._sequence.visibleRange(start, end);
  }

  _visibleElementAt(position: number): TextElement | undefined {
    return this._sequence.atVisible(position);
  }

  _insertVisible(
    position: number,
    characters: readonly string[],
    ids: readonly CodecId[],
    lamports: readonly number[],
  ): void {
    this._sequence.insertAtVisible(
      position,
      characters.map((value, index) => ({
        value,
        id: ids[index]!,
        lamport: lamports[index]!,
        deleted: false,
        originLeft: undefined,
        originRight: undefined,
      })),
    );
  }

  _insertValidatedSnapshotChunk(
    text: string,
    peers: readonly bigint[],
    counters: readonly number[],
    lamports: readonly number[],
  ): void {
    const span = TextSequenceSpan.fromValidatedSnapshotChunk(
      text,
      peers,
      counters,
      lamports,
    );
    if (this.#validatedSnapshotSpans !== undefined) {
      this.#validatedSnapshotSpans.push(span);
    } else {
      this._sequence.insertSpanAtPhysical(this._sequence.allLength, span);
    }
  }

  _beginValidatedSnapshotLoad(): void {
    if (this.#validatedSnapshotSpans !== undefined) {
      throw new Error("text snapshot load is already active");
    }
    this.#validatedSnapshotSpans = [];
  }

  _endValidatedSnapshotLoad(): void {
    const spans = this.#validatedSnapshotSpans;
    if (spans === undefined) throw new Error("text snapshot load is not active");
    this.#validatedSnapshotSpans = undefined;
    this._sequence.loadValidatedSpans(spans);
  }

  _forEachVisibleSnapshotData(
    appendText: (text: string, scalarLength: number) => void,
    appendId: (peer: bigint, counter: number, lamport: number) => void,
  ): void {
    this._sequence.forEachVisibleStorageRange((storage, start, end) => {
      if (storage instanceof TextSequenceSpan) {
        appendText(storage.textRange(start, end), end - start);
        for (let offset = start; offset < end; offset += 1) {
          appendId(
            storage.peerAt(offset),
            storage.counterAt(offset),
            storage.lamportAt(offset),
          );
        }
        return;
      }
      if (storage instanceof SequenceSpan) {
        throw new Error("unexpected text sequence span implementation");
      }
      const elements = Array.isArray(storage) ? storage : [storage];
      let text = "";
      for (let offset = start; offset < end; offset += 1) {
        const element = elements[offset]!;
        text += element.value;
        appendId(element.id.peer, element.id.counter, element.lamport);
      }
      appendText(text, end - start);
    });
  }

  _insertFugue(
    position: number,
    text: string,
    startId: CodecId,
    lamport: number,
    causalVersion: CausalVersion,
  ): void {
    const current = this._sequence.isFullyIncluded(causalVersion);
    const authored = current ? undefined : this._sequence.causalView(causalVersion);
    const authoredLength = current ? this._sequence.visibleLength : authored!.length;
    const authoredPosition = Math.min(position, authoredLength);
    const needsStyleElements = !this._styleIndex.isEmpty;
    const firstCodePoint = text.codePointAt(0)!;
    const singleScalar =
      text.length === 1 || (text.length === 2 && firstCodePoint > 0xffff);
    const useIdContext = current && !needsStyleElements && !singleScalar;
    const currentContext =
      current && !useIdContext
        ? this._sequence.visibleInsertionContext(authoredPosition)
        : undefined;
    const currentIdContext = useIdContext
      ? this._sequence.visibleInsertionIdContext(
          authoredPosition,
          this.#insertionIdContext,
        )
      : undefined;
    const authoredLeft = current
      ? currentContext?.left
      : authored!.at(authoredPosition - 1);
    const authoredRight = needsStyleElements
      ? current
        ? this._sequence.atVisible(authoredPosition)
        : authored!.at(authoredPosition)
      : undefined;
    const insertion =
      currentIdContext === undefined
        ? fugueInsertion(
            this._sequence,
            position,
            startId,
            causalVersion,
            true,
            currentContext ?? { current: false, left: authoredLeft },
          )
        : undefined;
    const insertIndex = currentIdContext?.startIndex ?? insertion!.insertIndex;
    let inherited: Map<string, TextStyleMeta> | undefined;
    if (needsStyleElements) {
      inherited = new Map();
      for (const [key, meta] of this._attributeMetasAt(authoredLeft, causalVersion)) {
        if ((meta.info & 0b100) !== 0) inherited.set(key, meta);
      }
      for (const [key, meta] of this._attributeMetasAt(authoredRight, causalVersion)) {
        if ((meta.info & 0b010) !== 0) inherited.set(key, meta);
      }
    }
    let insertedLength: number;
    if (singleScalar) {
      const originLeft =
        currentIdContext === undefined || currentIdContext.leftPeer === undefined
          ? insertion?.originLeft
          : {
              peer: currentIdContext.leftPeer,
              counter: currentIdContext.leftCounter,
            };
      const originRight =
        currentIdContext === undefined || currentIdContext.rightPeer === undefined
          ? insertion?.originRight
          : {
              peer: currentIdContext.rightPeer,
              counter: currentIdContext.rightCounter,
            };
      const element: TextElement = {
        value: text,
        id: { peer: startId.peer, counter: startId.counter },
        lamport,
        deleted: false,
        originLeft,
        originRight,
      };
      this._sequence.insertAtPhysical(insertIndex, [element]);
      if (insertion !== undefined) {
        recordFugueInsertion(this._sequence, insertion, [element.id]);
      }
      insertedLength = 1;
    } else {
      const span =
        currentIdContext === undefined
          ? TextSequenceSpan.fromText(
              text,
              startId,
              lamport,
              insertion!.originLeft,
              insertion!.originRight,
            )
          : TextSequenceSpan.fromTextOrigins(
              text,
              startId,
              lamport,
              currentIdContext.leftPeer,
              currentIdContext.leftCounter,
              currentIdContext.rightPeer,
              currentIdContext.rightCounter,
            );
      this._sequence.insertSpanAtPhysical(insertIndex, span);
      if (insertion !== undefined) {
        recordFugueInsertionRun(this._sequence, insertion, startId);
      }
      insertedLength = span.length;
    }
    if (inherited !== undefined && inherited.size > 0) {
      const insertedRun = [
        {
          start: { peer: startId.peer, counter: startId.counter },
          length: insertedLength,
        },
      ];
      for (const [key, meta] of inherited) {
        this._styleIndex.add(insertedRun, key, meta);
      }
    }
  }

  _deleteIdSpan(startId: CodecId, length: number, deletedBy?: CodecId): void {
    if (Math.abs(length) === 1) {
      const element = this._sequence.findById(startId);
      if (element === undefined) return;
      this._sequence.deleteElement(element, deletedBy);
      return;
    }
    this._sequence.deleteIdSpan(startId, length, deletedBy);
  }

  _applyMark(
    start: number,
    end: number,
    key: string,
    value: RuntimeValue,
    meta?: TextStyleMeta,
    causalVersion?: CausalVersion,
  ): void {
    const appliedMeta =
      meta ??
      ({
        startId: { peer: -1n, counter: this._detachedStyleCounter },
        lamport: this._detachedStyleCounter++,
        info: 0,
        value,
      } satisfies TextStyleMeta);
    this._styleIndex.add(this._styleRuns(start, end, causalVersion), key, appliedMeta);
    if (this._styleVersion !== undefined) {
      const next = new Map(this._styleVersion);
      next.set(
        appliedMeta.startId.peer,
        Math.max(
          next.get(appliedMeta.startId.peer) ?? 0,
          appliedMeta.startId.counter + 1,
        ),
      );
      this._styleVersion = next;
    }
  }

  _styleRuns(start: number, end: number, causalVersion?: CausalVersion): SequenceIdRun[] {
    if (causalVersion === undefined || this._sequence.isFullyIncluded(causalVersion)) {
      return this._sequence.visibleIdRuns(start, end);
    }
    return this._sequence.causalView(causalVersion).idRuns(start, end);
  }

  _attributeHistoryAt(
    element: TextElement,
    key: string,
  ): readonly TextStyleMeta[] | undefined {
    return this._styleIndex.historyAt(element.id, key);
  }

  _attributeMetasAt(
    element: TextElement | undefined,
    version: CausalVersion | undefined = this._styleVersion,
  ): ReadonlyMap<string, TextStyleMeta> {
    return element === undefined
      ? new Map()
      : this._styleIndex.metasAt(element.id, version);
  }

  _attributeMetasResolver(
    version: CausalVersion | undefined = this._styleVersion,
  ): (element: TextElement) => ReadonlyMap<string, TextStyleMeta> {
    const metasAt = this._styleIndex.resolver(version);
    return (element) => metasAt(element.id);
  }

  _attributesAt(element: TextElement): ReadonlyMap<string, RuntimeValue> {
    return this.#attributeValues(this._attributeMetasAt(element));
  }

  _setStyleVersion(version: CausalVersion | undefined): void {
    this._styleVersion = version === undefined ? undefined : new Map(version);
  }

  #attributeResolver(): (element: TextElement) => ReadonlyMap<string, RuntimeValue> {
    const metasAt = this._attributeMetasResolver();
    return (element) => this.#attributeValues(metasAt(element));
  }

  #attributeValues(
    metas: ReadonlyMap<string, TextStyleMeta>,
  ): ReadonlyMap<string, RuntimeValue> {
    const cached = this.#attributeValuesCache.get(metas);
    if (cached !== undefined) return cached;
    const attributes = new Map<string, RuntimeValue>();
    for (const [key, meta] of metas) {
      if (meta.value !== null) attributes.set(key, meta.value);
    }
    this.#attributeValuesCache.set(metas, attributes);
    return attributes;
  }

  _validateInsertPosition(position: number): number {
    const unicodePosition = this.convertPos(position, "utf16", "unicode");
    if (unicodePosition === undefined) {
      throw new RangeError(`text position ${position} is out of range`);
    }
    return unicodePosition;
  }

  _unicodePosition(position: number): number {
    const unicodePosition = this.convertPos(position, "utf16", "unicode");
    if (unicodePosition === undefined) {
      throw new RangeError(`text position ${position} is not on a UTF-16 boundary`);
    }
    return unicodePosition;
  }

  _reset(): void {
    this._sequence.reset();
    this.#validatedSnapshotSpans = undefined;
    this._detachedCounter = 0;
    this._detachedStyleCounter = 0;
    this._attributeHistoryComplete = true;
    this._styleIndex.reset();
    this._styleVersion = undefined;
  }
}

export class LoroMovableList<T = unknown> extends LoroList<T> {
  _valueHistoryComplete = true;
  _moveHistoryComplete = true;

  kind(): "MovableList" {
    return "MovableList";
  }

  move(from: number, to: number): void {
    validateIndex(from, this.length);
    if (!Number.isSafeInteger(to) || to < 0 || to >= this.length) {
      throw new RangeError(`movable-list destination ${to} is out of range`);
    }
    if (from === to) return;
    if (this._doc === undefined) {
      this._applyMove(from, to);
      return;
    }
    this._doc._movableMove(this, from, to);
  }

  mov(from: number, to: number): void {
    this.move(from, to);
  }

  set(pos: number, value: T): void {
    validateIndex(pos, this.length);
    if (isContainer(value))
      throw new TypeError("use setContainer() to attach a child container");
    if (this._doc === undefined) {
      this._sequence.atVisible(pos)!.value = normalizeDetachedValue(value);
      return;
    }
    this._doc._movableSet(this, pos, value);
  }

  setContainer<C extends Container>(pos: number, child: C): C {
    validateIndex(pos, this.length);
    if (this._doc === undefined) {
      const element = this._sequence.atVisible(pos)!;
      element.value = child;
      this._bindChildren([element]);
      return child;
    }
    return this._doc._movableSetContainer(this, pos, child);
  }

  getCreatorAt(pos: number): string | undefined {
    return this._sequence.atVisible(pos)?.id.peer.toString();
  }

  getLastMoverAt(pos: number): string | undefined {
    return this._sequence.atVisible(pos)?.id.peer.toString();
  }

  getLastEditorAt(pos: number): string | undefined {
    return this._sequence.atVisible(pos)?.id.peer.toString();
  }

  _applyMove(
    from: number,
    to: number,
    operation?: Pick<SequenceMoveMeta, "id" | "lamport">,
    replaceExisting = false,
  ): void {
    const element = this._sequence.atVisible(from);
    if (element === undefined) return;
    const beforePrevious = this._sequence.previousVisible(element)?.id;
    const beforeNext = this._sequence.nextVisible(element)?.id;
    this._sequence.moveVisible(from, to);
    if (operation === undefined) return;
    const meta: SequenceMoveMeta = {
      id: operation.id,
      lamport: operation.lamport,
      beforePrevious,
      beforeNext,
      afterPrevious: this._sequence.previousVisible(element)?.id,
      afterNext: this._sequence.nextVisible(element)?.id,
    };
    let history = element.moveHistory;
    if (history === undefined) {
      history = [];
      element.moveHistory = history;
    }
    const index = lowerBoundSequenceMoveMeta(history, meta);
    const existing = history[index];
    if (
      existing !== undefined &&
      existing.id.peer === meta.id.peer &&
      existing.id.counter === meta.id.counter
    ) {
      if (replaceExisting) history[index] = meta;
    } else {
      history.splice(index, 0, meta);
    }
  }

  _moveToAnchors(
    element: SequenceElement,
    previousId: CodecId | undefined,
    nextId: CodecId | undefined,
  ): void {
    if (element.deleted) return;
    if (nextId === undefined) {
      if (this._sequence.nextVisible(element) !== undefined) {
        this._sequence.moveBefore(element, undefined);
      }
      return;
    }
    const next = this._sequence.findById(nextId);
    if (next !== undefined && !next.deleted && next !== element) {
      this._sequence.moveBefore(element, next);
      return;
    }
    const previous =
      previousId === undefined ? undefined : this._sequence.findById(previousId);
    if (previous !== undefined && !previous.deleted && previous !== element) {
      const successor = this._sequence.nextVisible(previous);
      if (successor !== element) this._sequence.moveBefore(element, successor);
      return;
    }
    if (previousId === undefined) {
      const first = this._sequence.atVisible(0);
      if (first !== undefined && first !== element) {
        this._sequence.moveBefore(element, first);
      }
    }
  }

  _applySet(
    element: SequenceElement,
    value: RuntimeValue,
    meta?: SequenceValueMeta,
  ): void {
    if (meta === undefined) {
      element.value = value;
      this._bindChildren([element]);
      return;
    }
    let history = element.valueHistory;
    if (history === undefined) {
      history = [];
      element.valueHistory = history;
    }
    const index = lowerBoundSequenceValueMeta(history, meta);
    const existing = history[index];
    if (
      existing === undefined ||
      existing.id.peer !== meta.id.peer ||
      existing.id.counter !== meta.id.counter
    ) {
      history.splice(index, 0, meta);
    }
    const winner = history.at(-1)!;
    element.value = winner.value;
    this._bindChildren([element]);
  }

  override _reset(): void {
    super._reset();
    this._valueHistoryComplete = true;
    this._moveHistoryComplete = true;
  }
}

export class LoroCounter extends LoroContainer {
  _value = 0;

  kind(): "Counter" {
    return "Counter";
  }

  increment(value: number): void {
    if (!Number.isFinite(value)) throw new TypeError("counter increment must be finite");
    if (this._doc === undefined) {
      this._value += value;
      return;
    }
    this._doc._counterIncrement(this, value);
  }

  decrement(value: number): void {
    this.increment(-value);
  }

  get value(): number {
    return this._value;
  }

  getValue(): number {
    return this._value;
  }

  toJSON(): number {
    return this._value;
  }

  override getShallowValue(): number {
    return this._value;
  }

  _reset(): void {
    this._value = 0;
  }
}

export interface TreeNodeRecord {
  readonly id: CodecId;
  parent: CodecId | undefined;
  position: Uint8Array;
  deleted: boolean;
  writer: LastWriter;
  lastMoveId: CodecId;
  readonly data: LoroMap;
}

interface TreeJsonValue<T> {
  readonly id: TreeID;
  readonly parent: TreeID | null;
  readonly index: number;
  readonly fractional_index: string;
  readonly meta: T;
  readonly children: TreeJsonValue<T>[];
}

export class LoroTree<
  T extends Record<string, unknown> = Record<string, unknown>,
> extends LoroContainer {
  readonly _nodes = new Map<string, TreeNodeRecord>();
  readonly _children = new Map<string, OrderedIndex<TreeNodeRecord>>();
  _fractionalIndexEnabled = true;

  kind(): "Tree" {
    return "Tree";
  }

  createNode(parent?: TreeID, index?: number): LoroTreeNode<T> {
    if (this._doc === undefined)
      throw new Error("tree nodes can only be created on an attached tree");
    return this._doc._treeCreate(this, parent, index);
  }

  move(target: TreeID, parent?: TreeID, index?: number): void {
    if (this._doc === undefined)
      throw new Error("tree nodes can only be moved on an attached tree");
    this._doc._treeMove(this, target, parent, index);
  }

  delete(target: TreeID): void {
    if (this._doc === undefined) return;
    this._doc._treeDelete(this, target);
  }

  has(target: TreeID): boolean {
    return this._nodes.has(target);
  }

  contains(target: TreeID): boolean {
    return this.has(target);
  }

  isNodeDeleted(target: TreeID): boolean {
    return this._nodes.get(target)?.deleted ?? false;
  }

  enableFractionalIndex(jitter = 0): void {
    if (!Number.isSafeInteger(jitter) || jitter < 0 || jitter > 0xff) {
      throw new RangeError("fractional-index jitter must be an unsigned byte");
    }
    this._fractionalIndexEnabled = true;
  }

  disableFractionalIndex(): void {
    this._fractionalIndexEnabled = false;
  }

  isFractionalIndexEnabled(): boolean {
    return this._fractionalIndexEnabled;
  }

  getNodeByID(target: TreeID): LoroTreeNode<T> | undefined {
    const record = this._nodes.get(target);
    return record === undefined ? undefined : new LoroTreeNode(this, record.id);
  }

  getNodes(options: { withDeleted?: boolean } = {}): LoroTreeNode<T>[] {
    const output: LoroTreeNode<T>[] = [];
    for (const record of this._nodes.values()) {
      if (options.withDeleted === true || !record.deleted) {
        output.push(new LoroTreeNode<T>(this, record.id));
      }
    }
    return output;
  }

  nodes(): LoroTreeNode<T>[] {
    return this.getNodes();
  }

  roots(): LoroTreeNode<T>[] {
    return this._childrenOf(undefined).map(
      (record) => new LoroTreeNode<T>(this, record.id),
    );
  }

  toArray(): TreeNodeValue<T>[] {
    return this._childrenOf(undefined).map(
      (record, index) => this._recordToNodeValue(record, index) as TreeNodeValue<T>,
    );
  }

  toJSON(): TreeJsonValue<T>[] {
    return this._childrenOf(undefined).map(
      (record, index) => this._recordToValue(record, index) as TreeJsonValue<T>,
    );
  }

  override getShallowValue(): TreeNodeShallowValue[] {
    return this._childrenOf(undefined).map((record, index) =>
      this._recordToShallowValue(record, index),
    );
  }

  _childrenOf(parent: CodecId | undefined): TreeNodeRecord[] {
    return this._children.get(treeParentKey(parent))?.values() ?? [];
  }

  _nodeAt(parent: CodecId | undefined, index: number): LoroTreeNode<T> | undefined {
    const record = this._children.get(treeParentKey(parent))?.at(index);
    return record === undefined ? undefined : new LoroTreeNode<T>(this, record.id);
  }

  _rootJsonValueAt(index: number): TreeJsonValue<T> | undefined {
    const roots = this._children.get(treeParentKey(undefined));
    const normalized = index < 0 ? (roots?.size ?? 0) + index : index;
    const record = roots?.at(normalized);
    return record === undefined
      ? undefined
      : (this._recordToValue(record, normalized) as TreeJsonValue<T>);
  }

  _rootCount(): number {
    return this._children.get(treeParentKey(undefined))?.size ?? 0;
  }

  _rootJsonValuesRange(start: number, end: number): TreeJsonValue<T>[] {
    return (
      this._children.get(treeParentKey(undefined))?.valuesRange(start, end) ?? []
    ).map(
      (record, offset) => this._recordToValue(record, start + offset) as TreeJsonValue<T>,
    );
  }

  _positionFor(
    parent: CodecId | undefined,
    index?: number,
    exclude?: CodecId,
  ): Uint8Array {
    const children = this._children.get(treeParentKey(parent));
    const excluded =
      exclude === undefined ? undefined : this._nodes.get(formatTreeId(exclude));
    const excludedIndex =
      excluded === undefined ||
      excluded.deleted ||
      !sameOptionalId(excluded.parent, parent)
        ? undefined
        : children?.indexOf(excluded);
    const length = (children?.size ?? 0) - (excludedIndex === undefined ? 0 : 1);
    const position = index ?? length;
    if (!Number.isSafeInteger(position) || position < 0 || position > length) {
      throw new RangeError(`tree index ${position} is out of range`);
    }
    const siblingAt = (siblingIndex: number): TreeNodeRecord | undefined =>
      children?.at(
        excludedIndex !== undefined && siblingIndex >= excludedIndex
          ? siblingIndex + 1
          : siblingIndex,
      );
    return fractionalIndexBetween(
      position === 0 ? undefined : siblingAt(position - 1)!.position,
      position === length ? undefined : siblingAt(position)!.position,
    );
  }

  _setRecord(record: TreeNodeRecord): void {
    this._nodes.set(formatTreeId(record.id), record);
    record.data._setParentBinding(this, { kind: "tree", record });
    if (!record.deleted) this._childrenIndex(record.parent).add(record);
  }

  _updateRecord(
    record: TreeNodeRecord,
    parent: CodecId | undefined,
    position: Uint8Array,
    writer: LastWriter,
    lastMoveId: CodecId,
  ): void {
    if (!record.deleted) this._children.get(treeParentKey(record.parent))?.delete(record);
    record.parent = parent;
    record.position = position;
    record.deleted = false;
    record.writer = writer;
    record.lastMoveId = lastMoveId;
    this._childrenIndex(parent).add(record);
  }

  _deleteRecord(record: TreeNodeRecord, writer: LastWriter): void {
    if (!record.deleted) this._children.get(treeParentKey(record.parent))?.delete(record);
    record.deleted = true;
    record.writer = writer;
  }

  _removeRecord(record: TreeNodeRecord): void {
    if (!record.deleted) this._children.get(treeParentKey(record.parent))?.delete(record);
    record.deleted = true;
    this._nodes.delete(formatTreeId(record.id));
  }

  _indexOf(record: TreeNodeRecord): number {
    return this._children.get(treeParentKey(record.parent))?.indexOf(record) ?? -1;
  }

  _recordToValue(
    record: TreeNodeRecord,
    index = this._indexOf(record),
  ): TreeJsonValue<Record<string, unknown>> {
    return {
      id: formatTreeId(record.id),
      parent: record.parent === undefined ? null : formatTreeId(record.parent),
      index,
      fractional_index: bytesToHex(record.position).toUpperCase(),
      meta: record.data.toJSON(),
      children: this._childrenOf(record.id).map((child, childIndex) =>
        this._recordToValue(child, childIndex),
      ),
    };
  }

  _recordToNodeValue(
    record: TreeNodeRecord,
    index = this._indexOf(record),
  ): TreeNodeJSON<Record<string, unknown>> {
    return {
      id: formatTreeId(record.id),
      parent: record.deleted
        ? "2147483647@18446744073709551615"
        : record.parent === undefined
          ? undefined
          : formatTreeId(record.parent),
      index: record.deleted ? 0 : index,
      fractionalIndex: bytesToHex(record.position).toUpperCase() || "80",
      meta: record.data.toJSON(),
      children: this._childrenOf(record.id).map((child, childIndex) =>
        this._recordToNodeValue(child, childIndex),
      ),
    };
  }

  _recordToShallowValue(
    record: TreeNodeRecord,
    index = this._indexOf(record),
  ): TreeNodeShallowValue {
    return {
      id: formatTreeId(record.id),
      parent: record.deleted
        ? "2147483647@18446744073709551615"
        : record.parent === undefined
          ? null
          : formatTreeId(record.parent),
      index: record.deleted ? 0 : index,
      fractional_index: bytesToHex(record.position).toUpperCase(),
      meta: record.data.id,
      children: this._childrenOf(record.id).map((child, childIndex) =>
        this._recordToShallowValue(child, childIndex),
      ),
    };
  }

  _reset(): void {
    this._nodes.clear();
    this._children.clear();
  }

  private _childrenIndex(parent: CodecId | undefined): OrderedIndex<TreeNodeRecord> {
    const key = treeParentKey(parent);
    let index = this._children.get(key);
    if (index === undefined) {
      index = new OrderedIndex(compareTreeRecords);
      this._children.set(key, index);
    }
    return index;
  }
}

export class LoroTreeNode<T extends Record<string, unknown> = Record<string, unknown>> {
  readonly #tree: LoroTree<T>;
  readonly #id: CodecId;

  constructor(tree: LoroTree<T>, id: CodecId) {
    this.#tree = tree;
    this.#id = id;
  }

  free(): void {}

  get id(): TreeID {
    return formatTreeId(this.#id);
  }

  get data(): LoroMap<T> {
    return this.#record().data as LoroMap<T>;
  }

  createNode(index?: number): LoroTreeNode<T> {
    return this.#tree.createNode(this.id, index);
  }

  move(parent?: LoroTreeNode<T>, index?: number): void {
    this.#tree.move(this.id, parent?.id, index);
  }

  moveAfter(target: LoroTreeNode<T>): void {
    const parent = target.parent();
    this.move(parent, target.index() + 1);
  }

  moveBefore(target: LoroTreeNode<T>): void {
    this.move(target.parent(), target.index());
  }

  parent(): LoroTreeNode<T> | undefined {
    const parent = this.#record().parent;
    return parent === undefined ? undefined : new LoroTreeNode(this.#tree, parent);
  }

  children(): LoroTreeNode<T>[] {
    return this.#tree
      ._childrenOf(this.#id)
      .map((record) => new LoroTreeNode(this.#tree, record.id));
  }

  _childAt(index: number): LoroTreeNode<T> | undefined {
    return this.#tree._nodeAt(this.#id, index);
  }

  index(): number {
    const record = this.#record();
    return this.#tree._indexOf(record);
  }

  fractionalIndex(): string | undefined {
    return this.#tree._fractionalIndexEnabled
      ? bytesToHex(this.#record().position).toUpperCase()
      : undefined;
  }

  isDeleted(): boolean {
    return this.#record().deleted;
  }

  getLastMoveId(): { peer: string; counter: number } {
    const id = this.#record().lastMoveId;
    return { peer: id.peer.toString(), counter: id.counter };
  }

  creationId(): { peer: string; counter: number } {
    return { peer: this.#id.peer.toString(), counter: this.#id.counter };
  }

  creator(): string {
    return this.#id.peer.toString();
  }

  toJSON(): TreeNodeJSON<T> {
    return this.#tree._recordToNodeValue(this.#record()) as TreeNodeJSON<T>;
  }

  _codecId(): CodecId {
    return this.#id;
  }

  #record(): TreeNodeRecord {
    const record = this.#tree._nodes.get(this.id);
    if (record === undefined) throw new Error(`tree node ${this.id} does not exist`);
    return record;
  }
}

export class Cursor {
  readonly #containerId: ContainerID;
  readonly #id: CodecId | undefined;
  readonly #side: Side;
  readonly #originPosition: number;

  constructor(
    containerId: ContainerID,
    id: CodecId | undefined,
    side: Side,
    originPosition = 0,
  ) {
    this.#containerId = containerId;
    this.#id = id;
    this.#side = side;
    this.#originPosition = originPosition;
  }

  free(): void {}

  containerId(): ContainerID {
    return this.#containerId;
  }

  pos(): { peer: string; counter: number } | undefined {
    return this.#id === undefined
      ? undefined
      : { peer: this.#id.peer.toString(), counter: this.#id.counter };
  }

  side(): Side {
    return this.#side;
  }

  kind(): "Cursor" {
    return "Cursor";
  }

  encode(): Uint8Array {
    const writer = new PostcardWriter();
    writer.writeOption(this.#id, (output, id) => {
      output.writeU64(id.peer);
      output.writeI32(id.counter);
    });
    writePostcardCursorContainer(writer, parseContainerId(this.#containerId));
    writer.writeU32(cursorSideToVariant(this.#side));
    writer.writeUsize(this.#originPosition);
    return writer.toUint8Array();
  }

  static decode(bytes: Uint8Array): Cursor {
    const reader = new PostcardReader(bytes);
    const id = reader.readOption((input) => ({
      peer: input.readU64(),
      counter: input.readI32(),
    }));
    const containerId = readPostcardCursorContainer(reader);
    const side = cursorSideFromVariant(reader.readU32());
    const originPosition = reader.readUsize();
    reader.assertEnd();
    return new Cursor(formatContainerId(containerId), id, side, originPosition);
  }

  _idValue(): CodecId | undefined {
    return this.#id;
  }

  _originPositionValue(): number {
    return this.#originPosition;
  }
}

function writePostcardCursorContainer(
  writer: PostcardWriter,
  id: CodecContainerId,
): void {
  if (id.kind === "root") {
    writer.writeU32(0);
    writer.writeString(id.name);
  } else {
    writer.writeU32(1);
    writer.writeU64(id.peer);
    writer.writeI32(id.counter);
  }
  writer.writeU8(containerTypeToHistoricalByte(id.containerType));
}

function readPostcardCursorContainer(reader: PostcardReader): CodecContainerId {
  const variant = reader.readU32();
  if (variant === 0) {
    const name = reader.readString();
    const containerType = containerTypeFromHistoricalByte(reader.readU8());
    return { kind: "root", name, containerType };
  }
  if (variant === 1) {
    const peer = reader.readU64();
    const counter = reader.readI32();
    const containerType = containerTypeFromHistoricalByte(reader.readU8());
    return { kind: "normal", peer, counter, containerType };
  }
  throw new TypeError(`unsupported cursor container variant ${variant}`);
}

function cursorSideToVariant(side: Side): number {
  return side === -1 ? 0 : side === 0 ? 1 : 2;
}

function cursorSideFromVariant(variant: number): Side {
  if (variant === 0) return -1;
  if (variant === 1) return 0;
  if (variant === 2) return 1;
  throw new TypeError(`unsupported cursor side variant ${variant}`);
}

export type Container =
  | LoroMap
  | LoroList
  | LoroText
  | LoroMovableList
  | LoroTree
  | LoroCounter;

export function isContainer(value: unknown): value is Container {
  return value instanceof LoroContainer;
}

export function getType(value: unknown): ContainerType | "Json" {
  return isContainer(value) ? value.kind() : "Json";
}

export function runtimeValueToJson(value: RuntimeValue): unknown {
  if (isContainer(value)) return value.toJSON();
  if (value instanceof Uint8Array) return value.slice();
  if (Array.isArray(value)) return value.map(runtimeValueToJson);
  if (typeof value === "object" && value !== null) {
    const record = value as Record<string, RuntimeValue>;
    const output: Record<string, unknown> = {};
    for (const key in record) output[key] = runtimeValueToJson(record[key]!);
    return output;
  }
  return value;
}

export function runtimeValueToShallow(value: RuntimeValue): unknown {
  if (isContainer(value)) return value.id;
  if (value instanceof Uint8Array) return value.slice();
  if (Array.isArray(value)) return value.map(runtimeValueToShallow);
  if (typeof value === "object" && value !== null) {
    const record = value as Record<string, RuntimeValue>;
    const output: Record<string, unknown> = {};
    for (const key in record) output[key] = runtimeValueToShallow(record[key]!);
    return output;
  }
  return value;
}

export function containerDeepValueWithId(container: Container): unknown {
  return { cid: container.id, value: containerValueWithId(container) };
}

function containerValueWithId(container: Container): unknown {
  if (container instanceof LoroMap) {
    return Object.fromEntries(
      container
        .keys()
        .map((key) => [key, runtimeValueDeepWithId(container._entries.get(key)!.value!)]),
    );
  }
  if (container instanceof LoroText) return container.toString();
  if (container instanceof LoroCounter) return container.value;
  if (container instanceof LoroTree) {
    const visit = (record: TreeNodeRecord): unknown => {
      return {
        id: formatTreeId(record.id),
        parent: record.parent === undefined ? null : formatTreeId(record.parent),
        index: container._indexOf(record),
        fractional_index: bytesToHex(record.position).toUpperCase(),
        meta: containerDeepValueWithId(record.data),
        children: container._childrenOf(record.id).map(visit),
      };
    };
    return container._childrenOf(undefined).map(visit);
  }
  return container
    ._visibleElements()
    .map((element) => runtimeValueDeepWithId(element.value));
}

function runtimeValueDeepWithId(value: RuntimeValue): unknown {
  if (isContainer(value)) return containerDeepValueWithId(value);
  if (value instanceof Uint8Array) return value.slice();
  if (Array.isArray(value)) return value.map(runtimeValueDeepWithId);
  if (typeof value === "object" && value !== null) {
    const record = value as Record<string, RuntimeValue>;
    const output: Record<string, unknown> = {};
    for (const key in record) output[key] = runtimeValueDeepWithId(record[key]!);
    return output;
  }
  return value;
}

export function cloneRuntimeValue(value: RuntimeValue | undefined): unknown {
  if (value === undefined) return undefined;
  if (isContainer(value)) return value;
  if (value instanceof Uint8Array) return value.slice();
  if (Array.isArray(value)) return value.map((item) => cloneRuntimeValue(item));
  if (typeof value === "object" && value !== null) {
    const record = value as Record<string, RuntimeValue>;
    const output: Record<string, unknown> = {};
    for (const key in record) output[key] = cloneRuntimeValue(record[key]!);
    return output;
  }
  return value;
}

function normalizeDetachedValue(value: unknown): RuntimeValue {
  if (value === undefined) return null;
  if (value === null || typeof value === "string" || typeof value === "boolean")
    return value;
  if (typeof value === "number") {
    if (!Number.isFinite(value))
      throw new TypeError("Loro values must be finite numbers");
    return value;
  }
  if (value instanceof Uint8Array) return value.slice();
  if (Array.isArray(value)) return value.map(normalizeDetachedValue);
  if (isContainer(value)) return value;
  if (typeof value === "object") {
    if (Object.getOwnPropertySymbols(value).length > 0) {
      throw new TypeError("Object keys must be strings");
    }
    const output: Record<string, RuntimeValue> = {};
    for (const key of Object.keys(value)) {
      output[key] = normalizeDetachedValue((value as Record<string, unknown>)[key]);
    }
    return output;
  }
  throw new TypeError(`unsupported Loro value type: ${typeof value}`);
}

function insertFugueElements<T extends SequenceElement>(
  sequence: SequenceIndex<T>,
  position: number,
  inserted: T[],
  causalVersion: CausalVersion,
  useOriginIndex = true,
): void {
  if (inserted.length === 0) return;

  const insertion = fugueInsertion(
    sequence,
    position,
    inserted[0]!.id,
    causalVersion,
    useOriginIndex,
  );
  const { insertIndex, originLeft, originRight } = insertion;

  for (let index = 0; index < inserted.length; index += 1) {
    inserted[index]!.originLeft = index === 0 ? originLeft : inserted[index - 1]!.id;
    inserted[index]!.originRight = originRight;
  }

  sequence.insertAtPhysical(insertIndex, inserted);
  recordFugueInsertion(
    sequence,
    insertion,
    inserted.map((element) => element.id),
  );
}

interface FugueOriginEntry {
  readonly id: CodecId;
  readonly originLeft: CodecId | undefined;
  readonly originRight: CodecId | undefined;
}

interface FugueOriginIndex {
  structureVersion: number;
  readonly explicitChildren: Map<string, FugueOriginEntry[]>;
}

interface FugueInsertionResult {
  insertIndex: number;
  originLeft: CodecId | undefined;
  originRight: CodecId | undefined;
  indexUpdate: FugueOriginIndex | undefined;
}

interface FuguePositionHint<T extends SequenceElement> {
  readonly current: boolean;
  readonly left: T | undefined;
  readonly startIndex?: number | undefined;
  readonly right?: T | undefined;
}

const fugueOriginIndexes = new WeakMap<object, FugueOriginIndex>();

// Callers consume a fugueInsertion result before the next insertion, so every
// path reuses this single fixed-shape record instead of allocating per edit.
const fugueInsertionResult: FugueInsertionResult = {
  insertIndex: 0,
  originLeft: undefined,
  originRight: undefined,
  indexUpdate: undefined,
};
const fugueScanVisited = new Map<bigint, Set<number>>();
const fugueCandidateEntries: FugueOriginEntry[] = [];
const fugueCandidateIndexes: number[] = [];
const fugueCandidateOrder: number[] = [];
const fugueImplicitEntry: {
  id: CodecId;
  originLeft: CodecId | undefined;
  originRight: CodecId | undefined;
} = { id: { peer: 0n, counter: 0 }, originLeft: undefined, originRight: undefined };

function compareFugueCandidateOrder(left: number, right: number): number {
  return fugueCandidateIndexes[left]! - fugueCandidateIndexes[right]!;
}

function fugueInsertion<T extends SequenceElement>(
  sequence: SequenceIndex<T>,
  position: number,
  insertedId: CodecId,
  causalVersion: CausalVersion,
  useOriginIndex = true,
  positionHint?: FuguePositionHint<T>,
): FugueInsertionResult {
  const current = positionHint?.current ?? sequence.isFullyIncluded(causalVersion);
  const currentContext =
    positionHint === undefined && current
      ? sequence.visibleInsertionContext(Math.min(position, sequence.visibleLength))
      : undefined;
  const authoredVisible =
    positionHint === undefined && !current
      ? sequence.causalView(causalVersion)
      : undefined;
  const authoredLength = current ? sequence.visibleLength : authoredVisible?.length;
  const authoredPosition = Math.min(position, authoredLength ?? position);
  const left =
    positionHint === undefined
      ? current
        ? currentContext?.left
        : authoredVisible?.at(authoredPosition - 1)
      : positionHint.left;
  const originLeft = left?.id;
  const startIndex =
    positionHint?.startIndex ??
    currentContext?.startIndex ??
    (left === undefined ? 0 : (sequence.physicalIndexOf(left) ?? -1) + 1);
  const currentRight = !current
    ? undefined
    : positionHint?.startIndex !== undefined
      ? positionHint.right
      : currentContext !== undefined
        ? currentContext.right
        : sequence.atPhysicalRaw(startIndex);
  const right = current
    ? currentRight === undefined
      ? undefined
      : { element: currentRight, index: startIndex }
    : sequence.findNextIncludedPhysical(startIndex, causalVersion);
  const originRightIndex = right?.index ?? sequence.allLength;
  const originRight = right?.element.id;

  // There is no concurrent/future interval to order. Avoid allocating and
  // maintaining an origin index for the overwhelmingly common local-edit path;
  // structureVersion will force a rebuild if a later merge needs the index.
  if (startIndex === originRightIndex) {
    fugueInsertionResult.insertIndex = startIndex;
    fugueInsertionResult.originLeft = originLeft;
    fugueInsertionResult.originRight = originRight;
    fugueInsertionResult.indexUpdate = undefined;
    return fugueInsertionResult;
  }

  if (useOriginIndex) {
    const indexed = indexedFugueInsertion(
      sequence,
      startIndex,
      originRightIndex,
      originLeft,
      originRight,
      insertedId,
    );
    if (indexed !== undefined) return indexed;
  }

  return scannedFugueInsertion(
    sequence,
    startIndex,
    originRightIndex,
    originLeft,
    originRight,
    insertedId,
  );
}

function scannedFugueInsertion<T extends SequenceElement>(
  sequence: SequenceIndex<T>,
  startIndex: number,
  originRightIndex: number,
  originLeft: CodecId | undefined,
  originRight: CodecId | undefined,
  insertedId: CodecId,
): FugueInsertionResult {
  const parentRightIndex = directRightParentIndex(sequence, originLeft, originRight);

  let insertIndex = startIndex;
  let scanning = false;
  fugueScanVisited.clear();
  for (let index = startIndex; index < originRightIndex; index += 1) {
    const other = sequence.atPhysicalRaw(index)!;
    if (
      !sameOptionalId(other.originLeft, originLeft) &&
      (other.originLeft === undefined ||
        fugueScanVisited.get(other.originLeft.peer)?.has(other.originLeft.counter) !==
          true)
    ) {
      break;
    }

    let visitedCounters = fugueScanVisited.get(other.id.peer);
    if (visitedCounters === undefined) {
      visitedCounters = new Set();
      fugueScanVisited.set(other.id.peer, visitedCounters);
    }
    visitedCounters.add(other.id.counter);
    if (sameOptionalId(other.originLeft, originLeft)) {
      if (sameOptionalId(other.originRight, originRight)) {
        if (other.id.peer > insertedId.peer) break;
        scanning = false;
      } else {
        const otherParentRightIndex = directRightParentIndex(
          sequence,
          originLeft,
          other.originRight,
        );
        const ordering = compareOptionalPositions(
          otherParentRightIndex,
          parentRightIndex,
        );
        if (ordering < 0) scanning = true;
        else if (ordering === 0 && other.id.peer > insertedId.peer) break;
        else scanning = false;
      }
    }

    if (!scanning) insertIndex = index + 1;
  }

  fugueInsertionResult.insertIndex = insertIndex;
  fugueInsertionResult.originLeft = originLeft;
  fugueInsertionResult.originRight = originRight;
  fugueInsertionResult.indexUpdate = undefined;
  return fugueInsertionResult;
}

function indexedFugueInsertion<T extends SequenceElement>(
  sequence: SequenceIndex<T>,
  startIndex: number,
  originRightIndex: number,
  originLeft: CodecId | undefined,
  originRight: CodecId | undefined,
  insertedId: CodecId,
): FugueInsertionResult | undefined {
  const originIndex = getFugueOriginIndex(sequence);
  // Reused candidate columns: gather the direct children inside the scanned
  // interval without per-call wrapper, slice, or filtered-copy allocations.
  fugueCandidateEntries.length = 0;
  fugueCandidateIndexes.length = 0;
  const explicit = originIndex.explicitChildren.get(optionalSequenceIdKey(originLeft));
  if (explicit !== undefined) {
    for (const entry of explicit) {
      const element = sequence.findByIdRaw(entry.id);
      const index = element === undefined ? undefined : sequence.physicalIndexOf(element);
      if (index === undefined) return undefined;
      if (index >= startIndex && index < originRightIndex) {
        fugueCandidateEntries.push(entry);
        fugueCandidateIndexes.push(index);
      }
    }
  }
  if (originLeft !== undefined) {
    // Consecutive IDs keep their single-child edge implicit; derive it on
    // lookup instead of recording an entry for every consecutive insert.
    const implicit = sequence.findByIdRaw({
      peer: originLeft.peer,
      counter: originLeft.counter + 1,
    });
    if (implicit !== undefined && sameOptionalId(implicit.originLeft, originLeft)) {
      const index = sequence.physicalIndexOf(implicit);
      if (index === undefined) return undefined;
      if (index >= startIndex && index < originRightIndex) {
        fugueImplicitEntry.id = implicit.id;
        fugueImplicitEntry.originLeft = originLeft;
        fugueImplicitEntry.originRight = implicit.originRight;
        fugueCandidateEntries.push(fugueImplicitEntry);
        fugueCandidateIndexes.push(index);
      }
    }
  }
  const count = fugueCandidateIndexes.length;
  fugueCandidateOrder.length = count;
  for (let index = 0; index < count; index += 1) fugueCandidateOrder[index] = index;
  fugueCandidateOrder.sort(compareFugueCandidateOrder);

  const parentRightIndex = directRightParentIndex(sequence, originLeft, originRight);
  let insertIndex = startIndex;
  let scanning = false;
  for (let orderIndex = 0; orderIndex < count; orderIndex += 1) {
    const candidate = fugueCandidateOrder[orderIndex]!;
    const other = fugueCandidateEntries[candidate]!;
    if (sameOptionalId(other.originRight, originRight)) {
      if (other.id.peer > insertedId.peer) break;
      scanning = false;
    } else {
      const otherParentRightIndex = directRightParentIndex(
        sequence,
        originLeft,
        other.originRight,
      );
      const ordering = compareOptionalPositions(otherParentRightIndex, parentRightIndex);
      if (ordering < 0) scanning = true;
      else if (ordering === 0 && other.id.peer > insertedId.peer) break;
      else scanning = false;
    }

    if (!scanning) {
      insertIndex =
        orderIndex + 1 < count
          ? fugueCandidateIndexes[fugueCandidateOrder[orderIndex + 1]!]!
          : originRightIndex;
    }
  }

  fugueInsertionResult.insertIndex = insertIndex;
  fugueInsertionResult.originLeft = originLeft;
  fugueInsertionResult.originRight = originRight;
  fugueInsertionResult.indexUpdate = originIndex;
  return fugueInsertionResult;
}

function getFugueOriginIndex<T extends SequenceElement>(
  sequence: SequenceIndex<T>,
): FugueOriginIndex {
  const cached = fugueOriginIndexes.get(sequence);
  if (cached?.structureVersion === sequence.structureVersion) return cached;

  // Consecutive IDs form the common single-child Fugue chain, so derive those
  // edges on lookup instead of allocating a map entry for every text scalar.
  // Any untracked physical edit changes structureVersion and rebuilds the
  // explicit branch index before it is queried again.
  const rebuilt: FugueOriginIndex = {
    structureVersion: sequence.structureVersion,
    explicitChildren: new Map(),
  };
  sequence.forEachPhysicalRaw((element) => {
    recordFugueOriginEntry(rebuilt, element.id, element.originLeft, element.originRight);
  });
  fugueOriginIndexes.set(sequence, rebuilt);
  return rebuilt;
}

function recordFugueInsertion<T extends SequenceElement>(
  sequence: SequenceIndex<T>,
  insertion: FugueInsertionResult,
  insertedIds: readonly CodecId[],
): void {
  const index = insertion.indexUpdate;
  if (index === undefined) return;
  let originLeft = insertion.originLeft;
  for (const id of insertedIds) {
    recordFugueOriginEntry(index, id, originLeft, insertion.originRight);
    originLeft = id;
  }
  index.structureVersion = sequence.structureVersion;
}

function recordFugueInsertionRun<T extends SequenceElement>(
  sequence: SequenceIndex<T>,
  insertion: FugueInsertionResult,
  startId: CodecId,
): void {
  const index = insertion.indexUpdate;
  if (index === undefined) return;
  recordFugueOriginEntry(index, startId, insertion.originLeft, insertion.originRight);
  index.structureVersion = sequence.structureVersion;
}

function recordFugueOriginEntry(
  index: FugueOriginIndex,
  id: CodecId,
  originLeft: CodecId | undefined,
  originRight: CodecId | undefined,
): void {
  if (
    originLeft !== undefined &&
    id.peer === originLeft.peer &&
    id.counter === originLeft.counter + 1
  ) {
    return;
  }
  const key = optionalSequenceIdKey(originLeft);
  const children = index.explicitChildren.get(key);
  const entry = {
    id: { peer: id.peer, counter: id.counter },
    originLeft:
      originLeft === undefined
        ? undefined
        : { peer: originLeft.peer, counter: originLeft.counter },
    originRight:
      originRight === undefined
        ? undefined
        : { peer: originRight.peer, counter: originRight.counter },
  };
  if (children === undefined) index.explicitChildren.set(key, [entry]);
  else children.push(entry);
}

function directRightParentIndex<T extends SequenceElement>(
  sequence: SequenceIndex<T>,
  originLeft: CodecId | undefined,
  originRight: CodecId | undefined,
): number | undefined {
  if (originRight === undefined) return undefined;
  const element = sequence.findByIdRaw(originRight);
  if (element === undefined) return undefined;
  const index = sequence.physicalIndexOf(element);
  return index !== undefined && sameOptionalId(element.originLeft, originLeft)
    ? index
    : undefined;
}

function compareOptionalPositions(
  left: number | undefined,
  right: number | undefined,
): number {
  if (left !== undefined && right !== undefined) return left - right;
  if (left !== undefined) return -1;
  if (right !== undefined) return 1;
  return 0;
}

function sequenceIdKey(id: CodecId): string {
  return `${id.peer}:${id.counter}`;
}

function optionalSequenceIdKey(id: CodecId | undefined): string {
  return id === undefined ? "root" : `id:${sequenceIdKey(id)}`;
}

function compareWriters(left: LastWriter, right: LastWriter): number {
  return (
    left.lamport - right.lamport ||
    (left.peer < right.peer ? -1 : left.peer > right.peer ? 1 : 0)
  );
}

function compareSequenceValueMeta(
  left: SequenceValueMeta,
  right: SequenceValueMeta,
): number {
  return compareWriters(
    { peer: left.id.peer, lamport: left.lamport },
    { peer: right.id.peer, lamport: right.lamport },
  );
}

function lowerBoundSequenceValueMeta(
  history: readonly SequenceValueMeta[],
  meta: SequenceValueMeta,
): number {
  let low = 0;
  let high = history.length;
  while (low < high) {
    const middle = (low + high) >>> 1;
    if (compareSequenceValueMeta(history[middle]!, meta) < 0) low = middle + 1;
    else high = middle;
  }
  return low;
}

function lowerBoundSequenceMoveMeta(
  history: readonly SequenceMoveMeta[],
  meta: SequenceMoveMeta,
): number {
  let low = 0;
  let high = history.length;
  while (low < high) {
    const middle = (low + high) >>> 1;
    const current = history[middle]!;
    const order = compareWriters(
      { peer: current.id.peer, lamport: current.lamport },
      { peer: meta.id.peer, lamport: meta.lamport },
    );
    if (order < 0) low = middle + 1;
    else high = middle;
  }
  return low;
}

function validateRange(position: number, length: number, total: number): void {
  if (
    !Number.isSafeInteger(position) ||
    !Number.isSafeInteger(length) ||
    position < 0 ||
    length < 0 ||
    position + length > total
  ) {
    throw new RangeError(
      `range ${position}..${position + length} is out of bounds for length ${total}`,
    );
  }
}

function validateIndex(index: number, length: number): void {
  if (!Number.isSafeInteger(index) || index < 0 || index >= length) {
    throw new RangeError(`index ${index} is out of range for length ${length}`);
  }
}

function sameOptionalId(left: CodecId | undefined, right: CodecId | undefined): boolean {
  return left === undefined || right === undefined
    ? left === right
    : left.peer === right.peer && left.counter === right.counter;
}

function comparePositions(left: Uint8Array, right: Uint8Array): number {
  const length = Math.min(left.length, right.length);
  for (let index = 0; index < length; index += 1) {
    if (left[index] !== right[index]) return left[index]! - right[index]!;
  }
  return left.length - right.length;
}

function compareTreeRecords(left: TreeNodeRecord, right: TreeNodeRecord): number {
  return (
    comparePositions(left.position, right.position) ||
    compareWriters(left.writer, right.writer) ||
    codecIdsCompare(left.id, right.id)
  );
}

function codecIdsCompare(left: CodecId, right: CodecId): number {
  return left.peer < right.peer
    ? -1
    : left.peer > right.peer
      ? 1
      : left.counter - right.counter;
}

function treeParentKey(parent: CodecId | undefined): string {
  return parent === undefined ? "root" : sequenceIdKey(parent);
}

function textElementsToDelta(
  elements: readonly TextElement[],
  attributesAt: (element: TextElement) => ReadonlyMap<string, RuntimeValue>,
): Delta<string>[] {
  const output: Delta<string>[] = [];
  let values: string[] = [];
  let attributes: ReadonlyMap<string, RuntimeValue> | undefined;
  const flush = (): void => {
    if (values.length === 0) return;
    const insert = values.join("");
    values = [];
    let jsonAttributes: Record<string, Value> | undefined;
    if (attributes !== undefined) {
      for (const [key, value] of attributes) {
        (jsonAttributes ??= {})[key] = runtimeValueToJson(value) as Value;
      }
    }
    if (jsonAttributes === undefined) output.push({ insert });
    else output.push({ insert, attributes: jsonAttributes });
  };
  for (const element of elements) {
    const nextAttributes = attributesAt(element);
    if (values.length > 0 && !textAttributesEqual(attributes, nextAttributes)) {
      flush();
    }
    attributes = nextAttributes;
    values.push(element.value);
  }
  flush();
  return output;
}

function textAttributesEqual(
  left: ReadonlyMap<string, RuntimeValue> | undefined,
  right: ReadonlyMap<string, RuntimeValue> | undefined,
): boolean {
  if (left === right) return true;
  if ((left?.size ?? 0) !== (right?.size ?? 0)) return false;
  for (const [key, value] of left ?? []) {
    if (right?.has(key) !== true || right.get(key) !== value) return false;
  }
  return true;
}

function isTextPosType(value: string): value is TextPosType {
  return value === "unicode" || value === "utf16" || value === "utf8";
}

function utf8CodePointLength(value: string): number {
  const codePoint = value.codePointAt(0) ?? 0;
  if (codePoint <= 0x7f) return 1;
  if (codePoint <= 0x7ff) return 2;
  if (codePoint <= 0xffff) return 3;
  return 4;
}

/** The code point whose UTF-16 end offset is `end`, ignoring pairs opened before `lowerBound`. */
function codePointBefore(text: string, end: number, lowerBound: number): number {
  const unit = text.charCodeAt(end - 1);
  if (unit >= 0xdc00 && unit <= 0xdfff && end - 2 >= lowerBound) {
    const high = text.charCodeAt(end - 2);
    if (high >= 0xd800 && high <= 0xdbff) {
      return 0x1_0000 + ((high - 0xd800) << 10) + (unit - 0xdc00);
    }
  }
  return unit;
}

function updateText(container: LoroText, target: string): void {
  const current = container.toString();
  const currentLength = current.length;
  const targetLength = target.length;
  // Common prefix/suffix measured in UTF-16 offsets while comparing by code
  // point, so astral characters are never split and no arrays are built.
  let prefixLength = 0;
  while (prefixLength < currentLength && prefixLength < targetLength) {
    const codePoint = current.codePointAt(prefixLength)!;
    if (codePoint !== target.codePointAt(prefixLength)) break;
    prefixLength += codePoint > 0xffff ? 2 : 1;
  }
  let suffixLength = 0;
  while (
    suffixLength < currentLength - prefixLength &&
    suffixLength < targetLength - prefixLength
  ) {
    const codePoint = codePointBefore(
      current,
      currentLength - suffixLength,
      prefixLength,
    );
    if (
      codePoint !== codePointBefore(target, targetLength - suffixLength, prefixLength)
    ) {
      break;
    }
    suffixLength += codePoint > 0xffff ? 2 : 1;
  }
  container.delete(prefixLength, currentLength - prefixLength - suffixLength);
  container.insert(
    prefixLength,
    target.slice(prefixLength, targetLength - suffixLength),
  );
}
