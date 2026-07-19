import { isContainer, LoroCounter, LoroList, LoroMap, LoroText, LoroTree, } from "./containers";
export function evaluateJsonPath(root, query) {
    const selectors = parseJsonPath(query);
    let current = [root];
    for (const selector of selectors) {
        current = selector.recursive
            ? applyRecursiveSelector(current, selector, root)
            : applySelector(current, selector, root);
    }
    return current;
}
const UNKNOWN_EVENT_INDEX = Symbol("unknownEventIndex");
export function compileJsonPathEventMatcher(query) {
    const steps = parseJsonPath(query).map((selector) => ({
        recursive: selector.recursive,
        selectors: selector.kind === "child"
            ? selector.keys.map((value) => ({ kind: "key", value }))
            : selector.kind === "indices"
                ? selector.indices.map((value) => value < 0
                    ? { kind: "wildcard", value: undefined }
                    : { kind: "index", value })
                : [{ kind: "wildcard", value: undefined }],
    }));
    // Positions stay within [0, steps.length], so membership checks on the
    // ping-pong arrays replace the former per-element Set + spread + sort.
    const positionsAfter = (path, extraElement) => {
        let positions = [0];
        let next = [];
        const elementCount = extraElement === undefined ? path.length : path.length + 1;
        for (let index = 0; index < elementCount; index += 1) {
            const element = index < path.length ? path[index] : extraElement;
            next.length = 0;
            for (const position of positions) {
                if (position >= steps.length) {
                    if (!next.includes(position))
                        next.push(position);
                    continue;
                }
                const step = steps[position];
                if (step.recursive && !next.includes(position))
                    next.push(position);
                if (step.selectors.some((selector) => eventSelectorMatches(selector, element)) &&
                    !next.includes(position + 1)) {
                    next.push(position + 1);
                }
            }
            if (next.length === 0)
                return next;
            const swap = positions;
            positions = next;
            next = swap;
        }
        return positions;
    };
    const mayMatch = (path) => steps.length === 0 ||
        positionsAfter(path).some((position) => position >= steps.length);
    return (event) => {
        const basePath = event.path;
        if (mayMatch(basePath))
            return true;
        if (event.diff.type === "map") {
            const basePositions = positionsAfter(basePath);
            if (basePositions.length === 0)
                return false;
            const passedWildcard = basePositions.some((position) => position > 0 &&
                steps[position - 1]?.selectors.some((selector) => selector.kind === "wildcard"));
            for (const key in event.diff.updated) {
                if (passedWildcard || positionsAfter(basePath, key).length > 0)
                    return true;
            }
            return false;
        }
        if (event.diff.type === "list" ||
            event.diff.type === "tree" ||
            event.diff.type === "counter") {
            return positionsAfter(basePath, UNKNOWN_EVENT_INDEX).length > 0;
        }
        return false;
    };
}
function eventSelectorMatches(selector, element) {
    if (selector.kind === "wildcard")
        return true;
    if (selector.kind === "key")
        return typeof element === "string" && element === selector.value;
    return element === UNKNOWN_EVENT_INDEX || element === selector.value;
}
function parseJsonPath(query) {
    if (typeof query !== "string" || query.length === 0 || query[0] !== "$") {
        throw new TypeError("JSONPath must start with '$'");
    }
    const selectors = [];
    let index = 1;
    while (index < query.length) {
        let recursive = false;
        if (query.startsWith("..", index)) {
            recursive = true;
            index += 2;
        }
        else if (query[index] === ".") {
            index += 1;
        }
        else if (query[index] !== "[") {
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
        while (index < query.length && /[A-Za-z0-9_$-]/u.test(query[index])) {
            index += 1;
        }
        if (index === start)
            throw new SyntaxError(`missing JSONPath child at ${index}`);
        selectors.push({ kind: "child", keys: [query.slice(start, index)], recursive });
    }
    return selectors;
}
function readBracket(input, start) {
    let depth = 0;
    let quote;
    let escaped = false;
    for (let index = start; index < input.length; index += 1) {
        const character = input[index];
        if (quote !== undefined) {
            if (escaped)
                escaped = false;
            else if (character === "\\")
                escaped = true;
            else if (character === quote)
                quote = undefined;
            continue;
        }
        if (character === "'" || character === '"') {
            quote = character;
            continue;
        }
        if (character === "[")
            depth += 1;
        else if (character === "]") {
            depth -= 1;
            if (depth === 0) {
                return { content: input.slice(start + 1, index), end: index + 1 };
            }
        }
    }
    throw new SyntaxError(`unterminated JSONPath bracket at ${start}`);
}
function parseBracketSelector(content, recursive) {
    const trimmed = content.trim();
    if (trimmed === "*")
        return { kind: "wildcard", recursive };
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
        const parsePart = (part) => {
            const value = part.trim();
            if (value === "")
                return undefined;
            if (!/^-?\d+$/u.test(value))
                throw new SyntaxError("invalid JSONPath slice");
            return Number(value);
        };
        return {
            kind: "slice",
            start: parsePart(slices[0]),
            end: parsePart(slices[1]),
            step: slices[2] === undefined ? undefined : parsePart(slices[2]),
            recursive,
        };
    }
    throw new SyntaxError(`unsupported JSONPath selector [${content}]`);
}
function applyRecursiveSelector(values, selector, root) {
    const output = [];
    // `applySelector` never reads `selector.recursive`, so the selector passes
    // through unchanged instead of being cloned per visited node. The ancestor
    // set is reused in place: add on the way down, delete on the way up, which
    // keeps the same path-scoped cycle detection.
    const ancestors = new Set();
    const holder = [undefined];
    const visit = (value) => {
        holder[0] = value;
        output.push(...applySelector(holder, selector, root));
        const object = objectIdentity(value);
        if (object === undefined) {
            for (const child of childValues(value))
                visit(child);
            return;
        }
        if (ancestors.has(object))
            return;
        ancestors.add(object);
        for (const child of childValues(value))
            visit(child);
        ancestors.delete(object);
    };
    for (const value of values)
        visit(value);
    return output;
}
function applySelector(values, selector, root) {
    const output = [];
    switch (selector.kind) {
        case "child":
            for (const value of values) {
                for (const key of selector.keys) {
                    const child = lookupChild(value, key);
                    if (child.found)
                        output.push(child.value);
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
                        if (index >= 0 && index < value.length)
                            output.push(value.get(index));
                    }
                    continue;
                }
                if (value instanceof LoroTree) {
                    for (const rawIndex of selector.indices) {
                        const selected = value._rootJsonValueAt(rawIndex);
                        if (selected !== undefined)
                            output.push(selected);
                    }
                    continue;
                }
                const children = sequenceValues(value);
                if (children === undefined)
                    continue;
                for (const rawIndex of selector.indices) {
                    const index = rawIndex < 0 ? children.length + rawIndex : rawIndex;
                    if (index >= 0 && index < children.length)
                        output.push(children[index]);
                }
            }
            return output;
        case "slice":
            for (const value of values) {
                if (value instanceof LoroList) {
                    output.push(...sliceIndexedValues(value.length, (index) => value.get(index), (start, end) => value._valuesRange(start, end), selector.start, selector.end, selector.step));
                    continue;
                }
                if (value instanceof LoroTree) {
                    output.push(...sliceIndexedValues(value._rootCount(), (index) => value._rootJsonValueAt(index), (start, end) => value._rootJsonValuesRange(start, end), selector.start, selector.end, selector.step));
                    continue;
                }
                const children = sequenceValues(value);
                if (children === undefined)
                    continue;
                output.push(...sliceValues(children, selector.start, selector.end, selector.step));
            }
            return output;
        case "filter": {
            const expression = new FilterParser(selector.expression, root);
            for (const value of values) {
                for (const child of childValues(value)) {
                    if (expression.test(child))
                        output.push(child);
                }
            }
            return output;
        }
    }
}
function lookupChild(value, key) {
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
        if (!/^-?\d+$/u.test(key))
            return { found: false, value: undefined };
        const rawIndex = Number(key);
        const index = rawIndex < 0 ? value.length + rawIndex : rawIndex;
        return index >= 0 && index < value.length
            ? { found: true, value: value.get(index) }
            : { found: false, value: undefined };
    }
    if (value instanceof LoroTree) {
        if (!/^-?\d+$/u.test(key))
            return { found: false, value: undefined };
        const selected = value._rootJsonValueAt(Number(key));
        return selected === undefined
            ? { found: false, value: undefined }
            : { found: true, value: selected };
    }
    if (Array.isArray(value)) {
        if (!/^-?\d+$/u.test(key))
            return { found: false, value: undefined };
        const rawIndex = Number(key);
        const index = rawIndex < 0 ? value.length + rawIndex : rawIndex;
        return index >= 0 && index < value.length
            ? { found: true, value: value[index] }
            : { found: false, value: undefined };
    }
    if (typeof value === "object" && value !== null && !isContainer(value)) {
        return Object.prototype.hasOwnProperty.call(value, key)
            ? { found: true, value: value[key] }
            : { found: false, value: undefined };
    }
    return { found: false, value: undefined };
}
function childValues(value) {
    if (value instanceof LoroMap)
        return value.values();
    if (value instanceof Map)
        return [...value.values()];
    if (value instanceof LoroList)
        return value.toArray();
    if (value instanceof LoroTree)
        return value.toJSON();
    if (Array.isArray(value))
        return [...value];
    if (typeof value === "object" && value !== null && !isContainer(value)) {
        return Object.values(value);
    }
    return [];
}
function sequenceValues(value) {
    if (value instanceof LoroList)
        return value.toArray();
    if (value instanceof LoroTree)
        return value.toJSON();
    return Array.isArray(value) ? [...value] : undefined;
}
function sliceValues(values, rawStart, rawEnd, rawStep) {
    const step = rawStep ?? 1;
    if (step === 0)
        return [];
    const length = values.length;
    const normalize = (value) => {
        const resolved = value < 0 ? length + value : value;
        return Math.max(step > 0 ? 0 : -1, Math.min(step > 0 ? length : length - 1, resolved));
    };
    const start = rawStart === undefined ? (step > 0 ? 0 : length - 1) : normalize(rawStart);
    const end = rawEnd === undefined ? (step > 0 ? length : -1) : normalize(rawEnd);
    const output = [];
    if (step > 0) {
        for (let index = start; index < end; index += step)
            output.push(values[index]);
    }
    else {
        for (let index = start; index > end; index += step)
            output.push(values[index]);
    }
    return output;
}
function sliceIndexedValues(length, at, range, rawStart, rawEnd, rawStep) {
    const step = rawStep ?? 1;
    if (step === 0)
        return [];
    const normalize = (value) => {
        const resolved = value < 0 ? length + value : value;
        return Math.max(step > 0 ? 0 : -1, Math.min(step > 0 ? length : length - 1, resolved));
    };
    const start = rawStart === undefined ? (step > 0 ? 0 : length - 1) : normalize(rawStart);
    const end = rawEnd === undefined ? (step > 0 ? length : -1) : normalize(rawEnd);
    if (step === 1)
        return range(start, end);
    const output = [];
    if (step > 0) {
        for (let index = start; index < end; index += step)
            output.push(at(index));
    }
    else {
        for (let index = start; index > end; index += step)
            output.push(at(index));
    }
    return output;
}
const COMPARISON_OPERATORS = ["==", "!=", "<", "<=", ">", ">=", "contains", "in"];
const DOUBLE_CHAR_OPERATORS = ["&&", "||", "==", "!=", "<=", ">="];
const SINGLE_CHAR_OPERATORS = ["!", "<", ">"];
const PUNCTUATION_CHARS = ["(", ")", "[", "]", ","];
const FILTER_PATH_TERMINATORS = [")", ",", "!", "<", ">", "="];
class FilterParser {
    #tokens;
    #root;
    #index = 0;
    #current;
    constructor(expression, root) {
        this.#tokens = tokenizeFilter(expression);
        this.#root = root;
    }
    test(current) {
        this.#index = 0;
        this.#current = current;
        const value = this.#parseOr();
        this.#expect("eof");
        return filterTruthy(value);
    }
    #parseOr() {
        let left = this.#parseAnd();
        while (this.#match("operator", "||")) {
            const right = this.#parseAnd();
            left = filterTruthy(left) || filterTruthy(right);
        }
        return left;
    }
    #parseAnd() {
        let left = this.#parseComparison();
        while (this.#match("operator", "&&")) {
            const right = this.#parseComparison();
            left = filterTruthy(left) && filterTruthy(right);
        }
        return left;
    }
    #parseComparison() {
        const left = this.#parseUnary();
        const token = this.#peek();
        if (token.kind !== "operator" ||
            !COMPARISON_OPERATORS.includes(token.value)) {
            return left;
        }
        this.#index += 1;
        return compareFilterValues(left, this.#parseUnary(), token.value);
    }
    #parseUnary() {
        if (this.#match("operator", "!"))
            return !filterTruthy(this.#parseUnary());
        return this.#parsePrimary();
    }
    #parsePrimary() {
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
            };
        }
        if (token.kind === "identifier") {
            this.#index += 1;
            const name = token.value;
            this.#expect("punctuation", "(");
            const args = [];
            if (!this.#match("punctuation", ")")) {
                do
                    args.push(this.#parseOr());
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
            const values = [];
            if (!this.#match("punctuation", "]")) {
                do
                    values.push(this.#parseOr());
                while (this.#match("punctuation", ","));
                this.#expect("punctuation", "]");
            }
            return values;
        }
        throw new SyntaxError(`unexpected filter token ${token.kind}:${token.value}`);
    }
    #peek() {
        return this.#tokens[this.#index];
    }
    #match(kind, value) {
        const token = this.#peek();
        if (token.kind !== kind || (value !== undefined && token.value !== value))
            return false;
        this.#index += 1;
        return true;
    }
    #expect(kind, value) {
        if (!this.#match(kind, value)) {
            const token = this.#peek();
            throw new SyntaxError(`expected ${value ?? kind}, got ${token.kind}:${JSON.stringify(token.value)}`);
        }
    }
}
function tokenizeFilter(expression) {
    const tokens = [];
    let index = 0;
    while (index < expression.length) {
        const character = expression[index];
        if (/\s/u.test(character)) {
            index += 1;
            continue;
        }
        const pair = expression.slice(index, index + 2);
        if (DOUBLE_CHAR_OPERATORS.includes(pair)) {
            tokens.push({ kind: "operator", value: pair });
            index += 2;
            continue;
        }
        if (SINGLE_CHAR_OPERATORS.includes(character)) {
            tokens.push({ kind: "operator", value: character });
            index += 1;
            continue;
        }
        if (PUNCTUATION_CHARS.includes(character)) {
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
            }
            else if (identifier === "null") {
                tokens.push({ kind: "literal", value: null });
            }
            else if (identifier === "contains" || identifier === "in") {
                tokens.push({ kind: "operator", value: identifier });
            }
            else {
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
function readFilterPath(expression, start) {
    let index = start + 1;
    let bracketDepth = 0;
    let quote;
    let escaped = false;
    while (index < expression.length) {
        const character = expression[index];
        if (quote !== undefined) {
            if (escaped)
                escaped = false;
            else if (character === "\\")
                escaped = true;
            else if (character === quote)
                quote = undefined;
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
        if (bracketDepth === 0 &&
            (/\s/u.test(character) || FILTER_PATH_TERMINATORS.includes(character))) {
            break;
        }
        index += 1;
    }
    return { value: expression.slice(start, index), end: index };
}
function readQuotedLiteral(input, start) {
    const quote = input[start];
    let escaped = false;
    for (let index = start + 1; index < input.length; index += 1) {
        const character = input[index];
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
function compareFilterValues(left, right, operator) {
    const leftValues = atomValues(left);
    const rightValues = atomValues(right);
    if (operator === "in") {
        const candidates = rightValues.flatMap((value) => collectionValues(value));
        return leftValues.some((value) => candidates.some((candidate) => primitiveCompare(value, candidate, "==")));
    }
    return leftValues.some((leftValue) => rightValues.some((rightValue) => primitiveCompare(leftValue, rightValue, operator)));
}
function primitiveCompare(left, right, operator) {
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
            if (typeof a === "number" && typeof b === "number")
                return a < b;
            if (typeof a === "string" && typeof b === "string")
                return a < b;
            return false;
        case "<=":
            if (typeof a === "number" && typeof b === "number")
                return a <= b;
            if (typeof a === "string" && typeof b === "string")
                return a <= b;
            return false;
        case ">":
            if (typeof a === "number" && typeof b === "number")
                return a > b;
            if (typeof a === "string" && typeof b === "string")
                return a > b;
            return false;
        case ">=":
            if (typeof a === "number" && typeof b === "number")
                return a >= b;
            if (typeof a === "string" && typeof b === "string")
                return a >= b;
            return false;
        default:
            throw new SyntaxError(`unsupported filter operator ${operator}`);
    }
}
function callFilterFunction(name, args) {
    if (name === "count") {
        if (args.length !== 1)
            throw new TypeError("count() expects one argument");
        return isPathResult(args[0])
            ? args[0].values.length
            : collectionValues(args[0]).length;
    }
    if (name === "value") {
        if (args.length !== 1)
            throw new TypeError("value() expects one argument");
        return isPathResult(args[0]) ? args[0].values[0] : args[0];
    }
    if (name === "length") {
        if (args.length !== 1)
            throw new TypeError("length() expects one argument");
        const value = isPathResult(args[0]) ? args[0].values[0] : args[0];
        if (typeof value === "string" || Array.isArray(value))
            return value.length;
        if (value instanceof LoroMap)
            return value.size;
        if (value instanceof Map)
            return value.size;
        if (value instanceof LoroList)
            return value.length;
        if (typeof value === "object" && value !== null)
            return Object.keys(value).length;
        return 0;
    }
    throw new TypeError(`unsupported JSONPath function ${name}()`);
}
function atomValues(value) {
    return isPathResult(value) ? [...value.values] : [value];
}
function collectionValues(value) {
    if (Array.isArray(value))
        return value;
    if (value instanceof LoroList)
        return value.toArray();
    if (value instanceof LoroMap)
        return value.values();
    if (value instanceof Map)
        return [...value.values()];
    return [value];
}
function filterTruthy(value) {
    if (isPathResult(value)) {
        return (value.values.length > 0 &&
            value.values.some((item) => Boolean(comparableValue(item))));
    }
    return Boolean(comparableValue(value));
}
function comparableValue(value) {
    if (value instanceof LoroText)
        return value.toString();
    if (value instanceof LoroCounter)
        return value.value;
    return value;
}
function isPathResult(value) {
    return (typeof value === "object" &&
        value !== null &&
        value.kind === "path-result");
}
function objectIdentity(value) {
    return typeof value === "object" && value !== null ? value : undefined;
}
function splitTopLevel(input, separator) {
    const parts = [];
    let start = 0;
    let squareDepth = 0;
    let roundDepth = 0;
    let quote;
    let escaped = false;
    for (let index = 0; index < input.length; index += 1) {
        const character = input[index];
        if (quote !== undefined) {
            if (escaped)
                escaped = false;
            else if (character === "\\")
                escaped = true;
            else if (character === quote)
                quote = undefined;
            continue;
        }
        if (character === "'" || character === '"')
            quote = character;
        else if (character === "[")
            squareDepth += 1;
        else if (character === "]")
            squareDepth -= 1;
        else if (character === "(")
            roundDepth += 1;
        else if (character === ")")
            roundDepth -= 1;
        else if (character === separator && squareDepth === 0 && roundDepth === 0) {
            parts.push(input.slice(start, index));
            start = index + 1;
        }
    }
    parts.push(input.slice(start));
    return parts;
}
function isQuoted(value) {
    return (value.length >= 2 &&
        ((value[0] === "'" && value.at(-1) === "'") ||
            (value[0] === '"' && value.at(-1) === '"')));
}
function parseQuotedString(value) {
    if (!isQuoted(value))
        throw new SyntaxError("invalid quoted JSONPath key");
    const quote = value[0];
    const body = value.slice(1, -1);
    if (quote === '"')
        return JSON.parse(value);
    const doubleQuoted = `"${body.replace(/"/gu, '\\"').replace(/\\'/gu, "'")}"`;
    return JSON.parse(doubleQuoted);
}
