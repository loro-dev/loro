import {
  LoroContainer,
  type Container,
  type LoroMovableList,
  type RuntimeValue,
} from "./containers";
import type {
  MovableElement,
  MovableElementId,
  MovableUndoElement,
} from "./movable-list";
import { runtimeValuesEqual } from "./movable-list";
import type { ContainerID } from "./types";

function key(id: MovableElementId): string {
  return `${id.lamport}@${id.peer}`;
}

function sameWriter(left: MovableElementId, right: MovableElementId): boolean {
  return left.peer === right.peer && left.lamport === right.lamport;
}

/** Private to one UndoManager, like Rust's element/container remap. */
export class MovableListUndoState {
  readonly containers = new Map<ContainerID, Container>();
  readonly #elements = new Map<string, MovableElementId>();

  clear(): void {
    this.containers.clear();
    this.#elements.clear();
  }

  resolve(id: MovableElementId): MovableElementId {
    let next = this.#elements.get(key(id));
    while (next !== undefined) {
      id = next;
      next = this.#elements.get(key(id));
    }
    return id;
  }

  element(list: LoroMovableList, id: MovableElementId): MovableElement | undefined {
    const resolved = this.resolve(id);
    const element = list._state.element(resolved.peer, resolved.lamport);
    return element !== undefined && list._state.isVisible(element) ? element : undefined;
  }

  recordRecreatedList(
    source: readonly MovableUndoElement[],
    list: LoroMovableList,
    containers: ReadonlyMap<ContainerID, Container>,
  ): void {
    const current = list._state.visibleElements();
    if (
      source.length !== current.length ||
      !source.every((e, i) => {
        let value = e.value;
        while (value instanceof LoroContainer && containers.has(value.id))
          value = containers.get(value.id)!;
        return runtimeValuesEqual(value, current[i]!.value);
      })
    )
      throw new Error("movable-list undo child revival does not match its source");
    source.forEach((e, i) => {
      const target = current[i]!;
      const id = this.resolve(e.id);
      if (!sameWriter(id, target))
        this.#elements.set(key(id), { peer: target.peer, lamport: target.lamport });
    });
  }

  /** Same edit order and left-first gap policy as Rust undo/movable_list.rs. */
  apply(
    list: LoroMovableList,
    before: readonly MovableUndoElement[],
    after: readonly MovableUndoElement[],
    restore: (position: number, value: RuntimeValue, set: boolean) => void,
  ): void {
    const old = new Map(before.map((e) => [key(e.id), e]));
    const changed = new Map(after.map((e) => [key(e.id), e]));
    for (const e of after) {
      if (old.has(key(e.id))) continue;
      const current = this.element(list, e.id);
      if (current !== undefined) list.delete(list._state.indexOf(current)!, 1);
    }
    const repositioned = new Set<string>();
    for (const e of before) {
      const previous = changed.get(key(e.id));
      const exists = this.element(list, e.id) !== undefined;
      if (
        (previous === undefined && !exists) ||
        (previous !== undefined && !sameWriter(previous.position, e.position) && exists)
      ) {
        repositioned.add(key(e.id));
      }
    }
    const right: (MovableElementId | undefined)[] = new Array(before.length);
    let next: MovableElementId | undefined;
    for (let i = before.length - 1; i >= 0; i -= 1) {
      right[i] = next;
      const e = before[i]!;
      if (!repositioned.has(key(e.id)) && this.element(list, e.id) !== undefined)
        next = e.id;
    }
    const index = (id: MovableElementId): number => {
      const element = this.element(list, id);
      if (element === undefined) throw new Error("movable-list undo anchor disappeared");
      return list._state.indexOf(element)!;
    };
    let left: MovableElementId | undefined;
    for (let i = 0; i < before.length; i += 1) {
      const e = before[i]!;
      const previous = changed.get(key(e.id));
      if (repositioned.has(key(e.id))) {
        const gap =
          left !== undefined
            ? index(left) + 1
            : right[i] !== undefined
              ? index(right[i]!)
              : 0;
        if (previous === undefined) {
          restore(gap, e.value, false);
          const created = list._state.elementAt(gap)!;
          this.#elements.set(key(this.resolve(e.id)), {
            peer: created.peer,
            lamport: created.lamport,
          });
        } else {
          const from = index(e.id);
          list.move(from, gap - (from < gap ? 1 : 0));
        }
      }
      const current = this.element(list, e.id);
      if (
        previous !== undefined &&
        !sameWriter(previous.valueWriter, e.valueWriter) &&
        current !== undefined
      ) {
        let value = e.value;
        while (value instanceof LoroContainer && this.containers.has(value.id))
          value = this.containers.get(value.id)!;
        if (!(value instanceof LoroContainer) || current.value !== value) {
          restore(list._state.indexOf(current)!, value, true);
        }
      }
      if (this.element(list, e.id) !== undefined) left = e.id;
    }
  }
}
