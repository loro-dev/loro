class UndoDeque {
    // Array + head offset: capacity is capped by maxUndoSteps, so a Map of
    // slot indexes only costs extra entries per push/pop.
    #items = [];
    #start = 0;
    get length() {
        return this.#items.length - this.#start;
    }
    push(item) {
        this.#items.push(item);
    }
    pop() {
        if (this.length === 0)
            return undefined;
        const item = this.#items.pop();
        if (this.length === 0)
            this.clear();
        return item;
    }
    peek() {
        return this.length === 0 ? undefined : this.#items[this.#items.length - 1];
    }
    trimFront(length) {
        while (this.length > length)
            this.#start += 1;
        if (this.#start === 0)
            return;
        if (this.#start >= this.#items.length) {
            this.clear();
        }
        else if (this.#start >= 32 && this.#start * 2 >= this.#items.length) {
            // Release the trimmed slots once they dominate the backing array.
            this.#items.copyWithin(0, this.#start);
            this.#items.length -= this.#start;
            this.#start = 0;
        }
    }
    clear() {
        this.#items.length = 0;
        this.#start = 0;
    }
}
const EMPTY_META = { value: null, cursors: [] };
export class UndoManager {
    #doc;
    #peer;
    #undo = new UndoDeque();
    #redo = [];
    #excludeOriginPrefixes = new Set();
    #remoteTargets = new Set();
    #mergeInterval;
    #maxUndoSteps;
    #onPush;
    #onPop;
    #applying = false;
    #groupDepth = 0;
    #unsubscribe;
    constructor(doc, config = {}) {
        this.#doc = doc;
        this.#peer = doc.peerIdStr;
        this.#mergeInterval = config.mergeInterval ?? 1000;
        this.#maxUndoSteps = config.maxUndoSteps ?? 100;
        this.#onPush = config.onPush;
        this.#onPop = config.onPop;
        for (const prefix of config.excludeOriginPrefixes ?? []) {
            this.#excludeOriginPrefixes.add(prefix);
        }
        this.#unsubscribe = doc.subscribe((event) => this.#record(event));
    }
    free() {
        this.destroy();
    }
    undo() {
        const item = this.#undo.pop();
        if (item === undefined)
            return false;
        try {
            const redo = this.#invert(item, true);
            if (redo !== undefined)
                this.#redo.push(redo);
        }
        catch (error) {
            this.#undo.push(item);
            throw error;
        }
        return true;
    }
    redo() {
        const item = this.#redo.pop();
        if (item === undefined)
            return false;
        try {
            const undo = this.#invert(item, false);
            if (undo !== undefined)
                this.#pushUndo(undo, false);
        }
        catch (error) {
            this.#redo.push(item);
            throw error;
        }
        return true;
    }
    peer() {
        return this.#peer;
    }
    groupStart() {
        this.#groupDepth += 1;
    }
    groupEnd() {
        if (this.#groupDepth > 0)
            this.#groupDepth -= 1;
    }
    canUndo() {
        return this.#undo.length > 0;
    }
    canRedo() {
        return this.#redo.length > 0;
    }
    topUndoValue() {
        return this.#undo.peek()?.meta.value;
    }
    topRedoValue() {
        return this.#redo.at(-1)?.meta.value;
    }
    setMaxUndoSteps(steps) {
        if (!Number.isSafeInteger(steps) || steps < 0) {
            throw new RangeError("max undo steps must be a nonnegative integer");
        }
        this.#maxUndoSteps = steps;
        this.#trimUndo();
    }
    setMergeInterval(interval) {
        if (!Number.isFinite(interval) || interval < 0) {
            throw new RangeError("merge interval must be nonnegative");
        }
        this.#mergeInterval = interval;
    }
    addExcludeOriginPrefix(prefix) {
        this.#excludeOriginPrefixes.add(prefix);
    }
    setOnPush(callback) {
        this.#onPush = callback;
    }
    setOnPop(callback) {
        this.#onPop = callback;
    }
    clear() {
        this.#undo.clear();
        this.#redo.length = 0;
        this.#remoteTargets.clear();
    }
    clearUndo() {
        this.#undo.clear();
    }
    clearRedo() {
        this.#redo.length = 0;
    }
    destroy() {
        this.#unsubscribe();
        this.clear();
    }
    #record(event) {
        if (this.#applying)
            return;
        const targets = new Set();
        for (const { target } of event.events)
            targets.add(target);
        if (event.by === "checkout") {
            this.clear();
            return;
        }
        if (event.by === "import") {
            for (const target of targets)
                this.#remoteTargets.add(target);
            return;
        }
        if (event.origin !== undefined && this.#excludedOrigin(event.origin)) {
            for (const target of targets)
                this.#remoteTargets.add(target);
            return;
        }
        const spans = this.#doc.findIdSpansBetween(event.from, event.to).forward;
        const span = spans.find(({ peer }) => peer === this.#doc.peerIdStr) ??
            (spans.length === 1 ? spans[0] : undefined);
        if (span === undefined || span.length === 0)
            return;
        this.#peer = span.peer;
        const now = Date.now();
        const item = {
            peer: span.peer,
            range: { start: span.counter, end: span.counter + span.length },
            meta: this.#onPush?.(true, { start: span.counter, end: span.counter + span.length }, event) ?? EMPTY_META,
            timestamp: now,
            targets,
        };
        const previous = this.#undo.peek();
        let conflictsWithRemote = false;
        if (previous !== undefined) {
            for (const target of previous.targets) {
                if (this.#remoteTargets.has(target)) {
                    conflictsWithRemote = true;
                    break;
                }
            }
        }
        const merge = previous !== undefined &&
            previous.peer === item.peer &&
            previous.range.end === item.range.start &&
            !conflictsWithRemote &&
            (this.#groupDepth > 0 ||
                (this.#mergeInterval > 0 && now - previous.timestamp < this.#mergeInterval));
        if (merge) {
            previous.range = { start: previous.range.start, end: item.range.end };
            previous.meta = item.meta;
            previous.timestamp = now;
            for (const target of item.targets)
                previous.targets.add(target);
        }
        else {
            this.#pushUndo(item, false);
        }
        this.#redo.length = 0;
        this.#remoteTargets.clear();
    }
    #invert(item, isUndo) {
        const before = this.#doc.frontiers();
        this.#applying = true;
        try {
            this.#doc._undoIdSpan(item.peer, item.range);
            this.#doc.commit({ origin: isUndo ? "undo" : "redo" });
            const after = this.#doc.frontiers();
            const spans = this.#doc.findIdSpansBetween(before, after).forward;
            const span = spans.find(({ peer }) => peer === this.#doc.peerIdStr) ??
                (spans.length === 1 ? spans[0] : undefined);
            const poppedMeta = {
                value: item.meta.value,
                cursors: this.#doc._transformUndoCursors(item.meta.cursors),
            };
            this.#onPop?.(isUndo, poppedMeta, item.range);
            if (span === undefined || span.length === 0)
                return undefined;
            const range = { start: span.counter, end: span.counter + span.length };
            this.#peer = span.peer;
            return {
                peer: span.peer,
                range,
                meta: this.#onPush?.(!isUndo, range) ?? EMPTY_META,
                timestamp: Date.now(),
                targets: new Set(item.targets),
            };
        }
        finally {
            this.#applying = false;
        }
    }
    #pushUndo(item, clearRedo) {
        this.#undo.push(item);
        this.#trimUndo();
        if (clearRedo)
            this.#redo.length = 0;
    }
    #excludedOrigin(origin) {
        if (this.#excludeOriginPrefixes.size === 0)
            return false;
        for (const prefix of this.#excludeOriginPrefixes) {
            if (origin.startsWith(prefix))
                return true;
        }
        return false;
    }
    #trimUndo() {
        this.#undo.trimFront(this.#maxUndoSteps);
    }
}
