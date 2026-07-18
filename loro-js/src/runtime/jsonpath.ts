import {
  isContainer,
  LoroCounter,
  LoroList,
  LoroMap,
  LoroText,
  LoroTree,
} from "./containers";
import type { LoroEvent } from "./types";

type Selector =
  | {
      readonly kind: "child";
      readonly keys: readonly string[];
      readonly recursive: boolean;
    }
  | { readonly kind: "wildcard"; readonly recursive: boolean }
  | {
      readonly kind: "indices";
      readonly indices: readonly number[];
      readonly recursive: boolean;
    }
  | {
      readonly kind: "slice";
      readonly start: number | undefined;
      readonly end: number | undefined;
      readonly step: number | undefined;
      readonly recursive: boolean;
    }
  | { readonly kind: "filter"; readonly expression: string; readonly recursive: boolean };

interface LookupResult {
  readonly found: boolean;
  readonly value: unknown;
}

interface PathResult {
  readonly kind: "path-result";
  readonly values: readonly unknown[];
}

export function evaluateJsonPath(root: unknown, query: string): unknown[] {
  const selectors = parseJsonPath(query);
  let current = [root];
  for (const selector of selectors) {
    current = selector.recursive
      ? applyRecursiveSelector(current, selector, root)
      : applySelector(current, selector, root);
  }
  return current;
}

type EventMatchSelector =
  | { readonly kind: "key"; readonly value: string }
  | { readonly kind: "index"; readonly value: number }
  | { readonly kind: "wildcard" };

interface EventMatchStep {
  readonly recursive: boolean;
  readonly selectors: readonly EventMatchSelector[];
}

const UNKNOWN_EVENT_INDEX = Symbol("unknownEventIndex");
type EventPathElement = string | number | typeof UNKNOWN_EVENT_INDEX;

export function compileJsonPathEventMatcher(
  query: string,
): (event: LoroEvent) => boolean {
  const steps = parseJsonPath(query).map<EventMatchStep>((selector) => ({
    recursive: selector.recursive,
    selectors:
      selector.kind === "child"
        ? selector.keys.map((value) => ({ kind: "key", value }))
        : selector.kind === "indices"
          ? selector.indices.map((value) =>
              value < 0
                ? ({ kind: "wildcard" } as const)
                : ({ kind: "index", value } as const),
            )
          : [{ kind: "wildcard" }],
  }));
  const positionsAfter = (path: readonly EventPathElement[]): number[] => {
    let positions = [0];
    for (const element of path) {
      const next = new Set<number>();
      for (const position of positions) {
        if (position >= steps.length) {
          next.add(position);
          continue;
        }
        const step = steps[position]!;
        if (step.recursive) next.add(position);
        if (step.selectors.some((selector) => eventSelectorMatches(selector, element))) {
          next.add(position + 1);
        }
      }
      positions = [...next].sort((left, right) => left - right);
      if (positions.length === 0) break;
    }
    return positions;
  };
  const mayMatch = (path: readonly EventPathElement[]): boolean =>
    steps.length === 0 ||
    positionsAfter(path).some((position) => position >= steps.length);

  return (event) => {
    const basePath = event.path as readonly EventPathElement[];
    if (mayMatch(basePath)) return true;
    if (event.diff.type === "map") {
      const basePositions = positionsAfter(basePath);
      if (basePositions.length === 0) return false;
      const passedWildcard = basePositions.some(
        (position) =>
          position > 0 &&
          steps[position - 1]?.selectors.some((selector) => selector.kind === "wildcard"),
      );
      return Object.keys(event.diff.updated).some(
        (key) => positionsAfter([...basePath, key]).length > 0 || passedWildcard,
      );
    }
    if (
      event.diff.type === "list" ||
      event.diff.type === "tree" ||
      event.diff.type === "counter"
    ) {
      return positionsAfter([...basePath, UNKNOWN_EVENT_INDEX]).length > 0;
    }
    return false;
  };
}

function eventSelectorMatches(
  selector: EventMatchSelector,
  element: EventPathElement,
): boolean {
  if (selector.kind === "wildcard") return true;
  if (selector.kind === "key")
    return typeof element === "string" && element === selector.value;
  return element === UNKNOWN_EVENT_INDEX || element === selector.value;
}

function parseJsonPath(query: string): Selector[] {
  if (typeof query !== "string" || query.length === 0 || query[0] !== "$") {
    throw new TypeError("JSONPath must start with '$'");
  }
  const selectors: Selector[] = [];
  let index = 1;
  while (index < query.length) {
    let recursive = false;
    if (query.startsWith("..", index)) {
      recursive = true;
      index += 2;
    } else if (query[index] === ".") {
      index += 1;
    } else if (query[index] !== "[") {
      throw new SyntaxError(`unexpected JSONPath token at ${index}`);
    }

    if (query[index] === "[") {
      const bracket = readBracket(query, index);
      selectors.push(parseBracketSelector(bracket.content, recursive));
      index = bracket.end;
      continue;
    }
    if (query[index] === "*") {
      selectors.push({ kind: "wildcard", recursive });
      index += 1;
      continue;
    }
    const start = index;
    while (index < query.length && /[A-Za-z0-9_$-]/u.test(query[index]!)) {
      index += 1;
    }
    if (index === start) throw new SyntaxError(`missing JSONPath child at ${index}`);
    selectors.push({ kind: "child", keys: [query.slice(start, index)], recursive });
  }
  return selectors;
}

function readBracket(
  input: string,
  start: number,
): { readonly content: string; readonly end: number } {
  let depth = 0;
  let quote: string | undefined;
  let escaped = false;
  for (let index = start; index < input.length; index += 1) {
    const character = input[index]!;
    if (quote !== undefined) {
      if (escaped) escaped = false;
      else if (character === "\\") escaped = true;
      else if (character === quote) quote = undefined;
      continue;
    }
    if (character === "'" || character === '"') {
      quote = character;
      continue;
    }
    if (character === "[") depth += 1;
    else if (character === "]") {
      depth -= 1;
      if (depth === 0) {
        return { content: input.slice(start + 1, index), end: index + 1 };
      }
    }
  }
  throw new SyntaxError(`unterminated JSONPath bracket at ${start}`);
}

function parseBracketSelector(content: string, recursive: boolean): Selector {
  const trimmed = content.trim();
  if (trimmed === "*") return { kind: "wildcard", recursive };
  if (trimmed.startsWith("?(") && trimmed.endsWith(")")) {
    return {
      kind: "filter",
      expression: trimmed.slice(2, -1),
      recursive,
    };
  }
  const parts = splitTopLevel(trimmed, ",");
  if (parts.length > 1) {
    if (parts.every((part) => isQuoted(part.trim()))) {
      return {
        kind: "child",
        keys: parts.map((part) => parseQuotedString(part.trim())),
        recursive,
      };
    }
    if (parts.every((part) => /^-?\d+$/u.test(part.trim()))) {
      return {
        kind: "indices",
        indices: parts.map((part) => Number(part.trim())),
        recursive,
      };
    }
    throw new SyntaxError("JSONPath union must contain only keys or only indexes");
  }
  if (isQuoted(trimmed)) {
    return { kind: "child", keys: [parseQuotedString(trimmed)], recursive };
  }
  if (/^-?\d+$/u.test(trimmed)) {
    return { kind: "indices", indices: [Number(trimmed)], recursive };
  }
  const slices = splitTopLevel(trimmed, ":");
  if (slices.length >= 2 && slices.length <= 3) {
    const parsePart = (part: string): number | undefined => {
      const value = part.trim();
      if (value === "") return undefined;
      if (!/^-?\d+$/u.test(value)) throw new SyntaxError("invalid JSONPath slice");
      return Number(value);
    };
    return {
      kind: "slice",
      start: parsePart(slices[0]!),
      end: parsePart(slices[1]!),
      step: slices[2] === undefined ? undefined : parsePart(slices[2]),
      recursive,
    };
  }
  throw new SyntaxError(`unsupported JSONPath selector [${content}]`);
}

function applyRecursiveSelector(
  values: readonly unknown[],
  selector: Selector,
  root: unknown,
): unknown[] {
  const output: unknown[] = [];
  const visit = (value: unknown, ancestors: ReadonlySet<object>): void => {
    output.push(...applySelector([value], { ...selector, recursive: false }, root));
    const object = objectIdentity(value);
    if (object !== undefined && ancestors.has(object)) return;
    const nextAncestors =
      object === undefined ? ancestors : new Set([...ancestors, object]);
    for (const child of childValues(value)) visit(child, nextAncestors);
  };
  for (const value of values) visit(value, new Set());
  return output;
}

function applySelector(
  values: readonly unknown[],
  selector: Selector,
  root: unknown,
): unknown[] {
  const output: unknown[] = [];
  switch (selector.kind) {
    case "child":
      for (const value of values) {
        for (const key of selector.keys) {
          const child = lookupChild(value, key);
          if (child.found) output.push(child.value);
        }
      }
      return output;
    case "wildcard":
      return values.flatMap(childValues);
    case "indices":
      for (const value of values) {
        if (value instanceof LoroList) {
          for (const rawIndex of selector.indices) {
            const index = rawIndex < 0 ? value.length + rawIndex : rawIndex;
            if (index >= 0 && index < value.length) output.push(value.get(index));
          }
          continue;
        }
        if (value instanceof LoroTree) {
          for (const rawIndex of selector.indices) {
            const selected = value._rootJsonValueAt(rawIndex);
            if (selected !== undefined) output.push(selected);
          }
          continue;
        }
        const children = sequenceValues(value);
        if (children === undefined) continue;
        for (const rawIndex of selector.indices) {
          const index = rawIndex < 0 ? children.length + rawIndex : rawIndex;
          if (index >= 0 && index < children.length) output.push(children[index]);
        }
      }
      return output;
    case "slice":
      for (const value of values) {
        if (value instanceof LoroList) {
          output.push(
            ...sliceIndexedValues(
              value.length,
              (index) => value.get(index),
              (start, end) => value._valuesRange(start, end),
              selector.start,
              selector.end,
              selector.step,
            ),
          );
          continue;
        }
        if (value instanceof LoroTree) {
          output.push(
            ...sliceIndexedValues(
              value._rootCount(),
              (index) => value._rootJsonValueAt(index),
              (start, end) => value._rootJsonValuesRange(start, end),
              selector.start,
              selector.end,
              selector.step,
            ),
          );
          continue;
        }
        const children = sequenceValues(value);
        if (children === undefined) continue;
        output.push(
          ...sliceValues(children, selector.start, selector.end, selector.step),
        );
      }
      return output;
    case "filter": {
      const expression = new FilterParser(selector.expression, root);
      for (const value of values) {
        for (const child of childValues(value)) {
          if (expression.test(child)) output.push(child);
        }
      }
      return output;
    }
  }
}

function lookupChild(value: unknown, key: string): LookupResult {
  if (value instanceof LoroMap) {
    const record = value._entries.get(key);
    if (record === undefined || record.deleted) {
      return { found: false, value: undefined };
    }
    return { found: true, value: value.get(key) };
  }
  if (value instanceof Map) {
    return value.has(key)
      ? { found: true, value: value.get(key) }
      : { found: false, value: undefined };
  }
  if (value instanceof LoroList) {
    if (!/^-?\d+$/u.test(key)) return { found: false, value: undefined };
    const rawIndex = Number(key);
    const index = rawIndex < 0 ? value.length + rawIndex : rawIndex;
    return index >= 0 && index < value.length
      ? { found: true, value: value.get(index) }
      : { found: false, value: undefined };
  }
  if (value instanceof LoroTree) {
    if (!/^-?\d+$/u.test(key)) return { found: false, value: undefined };
    const selected = value._rootJsonValueAt(Number(key));
    return selected === undefined
      ? { found: false, value: undefined }
      : { found: true, value: selected };
  }
  if (Array.isArray(value)) {
    if (!/^-?\d+$/u.test(key)) return { found: false, value: undefined };
    const rawIndex = Number(key);
    const index = rawIndex < 0 ? value.length + rawIndex : rawIndex;
    return index >= 0 && index < value.length
      ? { found: true, value: value[index] }
      : { found: false, value: undefined };
  }
  if (typeof value === "object" && value !== null && !isContainer(value)) {
    return Object.prototype.hasOwnProperty.call(value, key)
      ? { found: true, value: (value as Record<string, unknown>)[key] }
      : { found: false, value: undefined };
  }
  return { found: false, value: undefined };
}

function childValues(value: unknown): unknown[] {
  if (value instanceof LoroMap) return value.values();
  if (value instanceof Map) return [...value.values()];
  if (value instanceof LoroList) return value.toArray();
  if (value instanceof LoroTree) return value.toJSON();
  if (Array.isArray(value)) return [...value];
  if (typeof value === "object" && value !== null && !isContainer(value)) {
    return Object.values(value);
  }
  return [];
}

function sequenceValues(value: unknown): unknown[] | undefined {
  if (value instanceof LoroList) return value.toArray();
  if (value instanceof LoroTree) return value.toJSON();
  return Array.isArray(value) ? [...value] : undefined;
}

function sliceValues(
  values: readonly unknown[],
  rawStart: number | undefined,
  rawEnd: number | undefined,
  rawStep: number | undefined,
): unknown[] {
  const step = rawStep ?? 1;
  if (step === 0) return [];
  const length = values.length;
  const normalize = (value: number): number => {
    const resolved = value < 0 ? length + value : value;
    return Math.max(
      step > 0 ? 0 : -1,
      Math.min(step > 0 ? length : length - 1, resolved),
    );
  };
  const start =
    rawStart === undefined ? (step > 0 ? 0 : length - 1) : normalize(rawStart);
  const end = rawEnd === undefined ? (step > 0 ? length : -1) : normalize(rawEnd);
  const output: unknown[] = [];
  if (step > 0) {
    for (let index = start; index < end; index += step) output.push(values[index]);
  } else {
    for (let index = start; index > end; index += step) output.push(values[index]);
  }
  return output;
}

function sliceIndexedValues(
  length: number,
  at: (index: number) => unknown,
  range: (start: number, end: number) => unknown[],
  rawStart: number | undefined,
  rawEnd: number | undefined,
  rawStep: number | undefined,
): unknown[] {
  const step = rawStep ?? 1;
  if (step === 0) return [];
  const normalize = (value: number): number => {
    const resolved = value < 0 ? length + value : value;
    return Math.max(
      step > 0 ? 0 : -1,
      Math.min(step > 0 ? length : length - 1, resolved),
    );
  };
  const start =
    rawStart === undefined ? (step > 0 ? 0 : length - 1) : normalize(rawStart);
  const end = rawEnd === undefined ? (step > 0 ? length : -1) : normalize(rawEnd);
  if (step === 1) return range(start, end);
  const output: unknown[] = [];
  if (step > 0) {
    for (let index = start; index < end; index += step) output.push(at(index));
  } else {
    for (let index = start; index > end; index += step) output.push(at(index));
  }
  return output;
}

type FilterToken =
  | { readonly kind: "operator"; readonly value: string }
  | { readonly kind: "punctuation"; readonly value: string }
  | { readonly kind: "literal"; readonly value: unknown }
  | { readonly kind: "path"; readonly value: string }
  | { readonly kind: "identifier"; readonly value: string }
  | { readonly kind: "eof"; readonly value: "" };

class FilterParser {
  readonly #tokens: FilterToken[];
  readonly #root: unknown;
  #index = 0;
  #current: unknown;

  constructor(expression: string, root: unknown) {
    this.#tokens = tokenizeFilter(expression);
    this.#root = root;
  }

  test(current: unknown): boolean {
    this.#index = 0;
    this.#current = current;
    const value = this.#parseOr();
    this.#expect("eof");
    return filterTruthy(value);
  }

  #parseOr(): unknown {
    let left = this.#parseAnd();
    while (this.#match("operator", "||")) {
      const right = this.#parseAnd();
      left = filterTruthy(left) || filterTruthy(right);
    }
    return left;
  }

  #parseAnd(): unknown {
    let left = this.#parseComparison();
    while (this.#match("operator", "&&")) {
      const right = this.#parseComparison();
      left = filterTruthy(left) && filterTruthy(right);
    }
    return left;
  }

  #parseComparison(): unknown {
    const left = this.#parseUnary();
    const token = this.#peek();
    if (
      token.kind !== "operator" ||
      !["==", "!=", "<", "<=", ">", ">=", "contains", "in"].includes(token.value)
    ) {
      return left;
    }
    this.#index += 1;
    return compareFilterValues(left, this.#parseUnary(), token.value);
  }

  #parseUnary(): unknown {
    if (this.#match("operator", "!")) return !filterTruthy(this.#parseUnary());
    return this.#parsePrimary();
  }

  #parsePrimary(): unknown {
    const token = this.#peek();
    if (token.kind === "literal") {
      this.#index += 1;
      return token.value;
    }
    if (token.kind === "path") {
      this.#index += 1;
      const root = token.value.startsWith("@") ? this.#current : this.#root;
      const query = token.value.startsWith("@")
        ? `$${token.value.slice(1)}`
        : token.value;
      return {
        kind: "path-result",
        values: evaluateJsonPath(root, query),
      } satisfies PathResult;
    }
    if (token.kind === "identifier") {
      this.#index += 1;
      const name = token.value;
      this.#expect("punctuation", "(");
      const args: unknown[] = [];
      if (!this.#match("punctuation", ")")) {
        do args.push(this.#parseOr());
        while (this.#match("punctuation", ","));
        this.#expect("punctuation", ")");
      }
      return callFilterFunction(name, args);
    }
    if (this.#match("punctuation", "(")) {
      const value = this.#parseOr();
      this.#expect("punctuation", ")");
      return value;
    }
    if (this.#match("punctuation", "[")) {
      const values: unknown[] = [];
      if (!this.#match("punctuation", "]")) {
        do values.push(this.#parseOr());
        while (this.#match("punctuation", ","));
        this.#expect("punctuation", "]");
      }
      return values;
    }
    throw new SyntaxError(`unexpected filter token ${token.kind}:${token.value}`);
  }

  #peek(): FilterToken {
    return this.#tokens[this.#index]!;
  }

  #match(kind: FilterToken["kind"], value?: string): boolean {
    const token = this.#peek();
    if (token.kind !== kind || (value !== undefined && token.value !== value))
      return false;
    this.#index += 1;
    return true;
  }

  #expect(kind: FilterToken["kind"], value?: string): void {
    if (!this.#match(kind, value)) {
      const token = this.#peek();
      throw new SyntaxError(
        `expected ${value ?? kind}, got ${token.kind}:${JSON.stringify(token.value)}`,
      );
    }
  }
}

function tokenizeFilter(expression: string): FilterToken[] {
  const tokens: FilterToken[] = [];
  let index = 0;
  while (index < expression.length) {
    const character = expression[index]!;
    if (/\s/u.test(character)) {
      index += 1;
      continue;
    }
    const pair = expression.slice(index, index + 2);
    if (["&&", "||", "==", "!=", "<=", ">="].includes(pair)) {
      tokens.push({ kind: "operator", value: pair });
      index += 2;
      continue;
    }
    if (["!", "<", ">"].includes(character)) {
      tokens.push({ kind: "operator", value: character });
      index += 1;
      continue;
    }
    if (["(", ")", "[", "]", ","].includes(character)) {
      tokens.push({ kind: "punctuation", value: character });
      index += 1;
      continue;
    }
    if (character === "'" || character === '"') {
      const literal = readQuotedLiteral(expression, index);
      tokens.push({ kind: "literal", value: literal.value });
      index = literal.end;
      continue;
    }
    if (character === "@" || character === "$") {
      const path = readFilterPath(expression, index);
      tokens.push({ kind: "path", value: path.value });
      index = path.end;
      continue;
    }
    const number = expression.slice(index).match(/^-?(?:\d+(?:\.\d*)?|\.\d+)/u)?.[0];
    if (number !== undefined) {
      tokens.push({ kind: "literal", value: Number(number) });
      index += number.length;
      continue;
    }
    const identifier = expression.slice(index).match(/^[A-Za-z_][A-Za-z0-9_]*/u)?.[0];
    if (identifier !== undefined) {
      if (identifier === "true" || identifier === "false") {
        tokens.push({ kind: "literal", value: identifier === "true" });
      } else if (identifier === "null") {
        tokens.push({ kind: "literal", value: null });
      } else if (identifier === "contains" || identifier === "in") {
        tokens.push({ kind: "operator", value: identifier });
      } else {
        tokens.push({ kind: "identifier", value: identifier });
      }
      index += identifier.length;
      continue;
    }
    throw new SyntaxError(`invalid filter token at ${index}`);
  }
  tokens.push({ kind: "eof", value: "" });
  return tokens;
}

function readFilterPath(
  expression: string,
  start: number,
): { readonly value: string; readonly end: number } {
  let index = start + 1;
  let bracketDepth = 0;
  let quote: string | undefined;
  let escaped = false;
  while (index < expression.length) {
    const character = expression[index]!;
    if (quote !== undefined) {
      if (escaped) escaped = false;
      else if (character === "\\") escaped = true;
      else if (character === quote) quote = undefined;
      index += 1;
      continue;
    }
    if (character === "'" || character === '"') {
      quote = character;
      index += 1;
      continue;
    }
    if (character === "[") {
      bracketDepth += 1;
      index += 1;
      continue;
    }
    if (character === "]" && bracketDepth > 0) {
      bracketDepth -= 1;
      index += 1;
      continue;
    }
    if (
      bracketDepth === 0 &&
      (/\s/u.test(character) || [")", ",", "!", "<", ">", "="].includes(character))
    ) {
      break;
    }
    index += 1;
  }
  return { value: expression.slice(start, index), end: index };
}

function readQuotedLiteral(
  input: string,
  start: number,
): { readonly value: string; readonly end: number } {
  const quote = input[start]!;
  let escaped = false;
  for (let index = start + 1; index < input.length; index += 1) {
    const character = input[index]!;
    if (escaped) {
      escaped = false;
      continue;
    }
    if (character === "\\") {
      escaped = true;
      continue;
    }
    if (character === quote) {
      return {
        value: parseQuotedString(input.slice(start, index + 1)),
        end: index + 1,
      };
    }
  }
  throw new SyntaxError(`unterminated string literal at ${start}`);
}

function compareFilterValues(left: unknown, right: unknown, operator: string): boolean {
  const leftValues = atomValues(left);
  const rightValues = atomValues(right);
  if (operator === "in") {
    const candidates = rightValues.flatMap((value) => collectionValues(value));
    return leftValues.some((value) =>
      candidates.some((candidate) => primitiveCompare(value, candidate, "==")),
    );
  }
  return leftValues.some((leftValue) =>
    rightValues.some((rightValue) => primitiveCompare(leftValue, rightValue, operator)),
  );
}

function primitiveCompare(left: unknown, right: unknown, operator: string): boolean {
  const a = comparableValue(left);
  const b = comparableValue(right);
  switch (operator) {
    case "==":
      return Object.is(a, b);
    case "!=":
      return !Object.is(a, b);
    case "contains":
      return typeof a === "string"
        ? typeof b === "string" && a.includes(b)
        : Array.isArray(a) && a.some((value) => Object.is(value, b));
    case "<":
      if (typeof a === "number" && typeof b === "number") return a < b;
      if (typeof a === "string" && typeof b === "string") return a < b;
      return false;
    case "<=":
      if (typeof a === "number" && typeof b === "number") return a <= b;
      if (typeof a === "string" && typeof b === "string") return a <= b;
      return false;
    case ">":
      if (typeof a === "number" && typeof b === "number") return a > b;
      if (typeof a === "string" && typeof b === "string") return a > b;
      return false;
    case ">=":
      if (typeof a === "number" && typeof b === "number") return a >= b;
      if (typeof a === "string" && typeof b === "string") return a >= b;
      return false;
    default:
      throw new SyntaxError(`unsupported filter operator ${operator}`);
  }
}

function callFilterFunction(name: string, args: readonly unknown[]): unknown {
  if (name === "count") {
    if (args.length !== 1) throw new TypeError("count() expects one argument");
    return isPathResult(args[0])
      ? args[0].values.length
      : collectionValues(args[0]).length;
  }
  if (name === "value") {
    if (args.length !== 1) throw new TypeError("value() expects one argument");
    return isPathResult(args[0]) ? args[0].values[0] : args[0];
  }
  if (name === "length") {
    if (args.length !== 1) throw new TypeError("length() expects one argument");
    const value = isPathResult(args[0]) ? args[0].values[0] : args[0];
    if (typeof value === "string" || Array.isArray(value)) return value.length;
    if (value instanceof LoroMap) return value.size;
    if (value instanceof Map) return value.size;
    if (value instanceof LoroList) return value.length;
    if (typeof value === "object" && value !== null) return Object.keys(value).length;
    return 0;
  }
  throw new TypeError(`unsupported JSONPath function ${name}()`);
}

function atomValues(value: unknown): unknown[] {
  return isPathResult(value) ? [...value.values] : [value];
}

function collectionValues(value: unknown): unknown[] {
  if (Array.isArray(value)) return value;
  if (value instanceof LoroList) return value.toArray();
  if (value instanceof LoroMap) return value.values();
  if (value instanceof Map) return [...value.values()];
  return [value];
}

function filterTruthy(value: unknown): boolean {
  if (isPathResult(value)) {
    return (
      value.values.length > 0 &&
      value.values.some((item) => Boolean(comparableValue(item)))
    );
  }
  return Boolean(comparableValue(value));
}

function comparableValue(value: unknown): unknown {
  if (value instanceof LoroText) return value.toString();
  if (value instanceof LoroCounter) return value.value;
  return value;
}

function isPathResult(value: unknown): value is PathResult {
  return (
    typeof value === "object" &&
    value !== null &&
    (value as PathResult).kind === "path-result"
  );
}

function objectIdentity(value: unknown): object | undefined {
  return typeof value === "object" && value !== null ? value : undefined;
}

function splitTopLevel(input: string, separator: string): string[] {
  const parts: string[] = [];
  let start = 0;
  let squareDepth = 0;
  let roundDepth = 0;
  let quote: string | undefined;
  let escaped = false;
  for (let index = 0; index < input.length; index += 1) {
    const character = input[index]!;
    if (quote !== undefined) {
      if (escaped) escaped = false;
      else if (character === "\\") escaped = true;
      else if (character === quote) quote = undefined;
      continue;
    }
    if (character === "'" || character === '"') quote = character;
    else if (character === "[") squareDepth += 1;
    else if (character === "]") squareDepth -= 1;
    else if (character === "(") roundDepth += 1;
    else if (character === ")") roundDepth -= 1;
    else if (character === separator && squareDepth === 0 && roundDepth === 0) {
      parts.push(input.slice(start, index));
      start = index + 1;
    }
  }
  parts.push(input.slice(start));
  return parts;
}

function isQuoted(value: string): boolean {
  return (
    value.length >= 2 &&
    ((value[0] === "'" && value.at(-1) === "'") ||
      (value[0] === '"' && value.at(-1) === '"'))
  );
}

function parseQuotedString(value: string): string {
  if (!isQuoted(value)) throw new SyntaxError("invalid quoted JSONPath key");
  const quote = value[0]!;
  const body = value.slice(1, -1);
  if (quote === '"') return JSON.parse(value) as string;
  const doubleQuoted = `"${body.replace(/"/gu, '\\"').replace(/\\'/gu, "'")}"`;
  return JSON.parse(doubleQuoted) as string;
}
