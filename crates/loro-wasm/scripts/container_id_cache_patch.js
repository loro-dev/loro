// A container handler never changes identity during one JS wrapper's lifetime.
// Attaching a detached handler returns a different wrapper, and getAttached()
// also returns a new wrapper. Cache on the wrapper, not by container id.
const __loroContainerIdCache = Symbol("loroContainerId");

function __loroCacheContainerIdGetter(ContainerClass) {
  const prototype = ContainerClass.prototype;
  const idDescriptor = Object.getOwnPropertyDescriptor(prototype, "id");
  const getId = idDescriptor && idDescriptor.get;
  const destroyDescriptor = Object.getOwnPropertyDescriptor(
    prototype,
    "__destroy_into_raw",
  );
  const destroy = destroyDescriptor && destroyDescriptor.value;

  if (!getId || !destroyDescriptor || !destroy) {
    throw new Error("Unexpected wasm-bindgen container wrapper shape");
  }

  Object.defineProperty(prototype, "id", {
    ...idDescriptor,
    get() {
      // Preserve wasm-bindgen's post-free error instead of returning stale data.
      if (this.__wbg_ptr === 0) {
        return getId.call(this);
      }

      const cached = this[__loroContainerIdCache];
      if (cached !== undefined) {
        return cached;
      }

      const id = getId.call(this);
      try {
        Object.defineProperty(this, __loroContainerIdCache, {
          configurable: true,
          value: id,
        });
      } catch {
        // Caching must not make a non-extensible wrapper's id unreadable.
      }
      return id;
    },
  });

  Object.defineProperty(prototype, "__destroy_into_raw", {
    ...destroyDescriptor,
    value() {
      try {
        delete this[__loroContainerIdCache];
      } catch {
        // Cache cleanup must not replace wasm-bindgen's free behavior.
      }
      return destroy.call(this);
    },
  });
}

// `kind()` returns a constant string per container class and never depends on
// instance state, so one module-level memo per class is enough. Repeated reads
// (e.g. loro-mirror's traversal) must not cross into WASM again.
function __loroCacheKindMethod(ContainerClass) {
  const prototype = ContainerClass.prototype;
  const kindDescriptor = Object.getOwnPropertyDescriptor(prototype, "kind");
  const kind = kindDescriptor && kindDescriptor.value;

  if (typeof kind !== "function") {
    throw new Error("Unexpected wasm-bindgen container wrapper shape");
  }

  let cached;
  Object.defineProperty(prototype, "kind", {
    ...kindDescriptor,
    value() {
      // Preserve wasm-bindgen's post-free error instead of returning stale data.
      if (this.__wbg_ptr === 0) {
        return kind.call(this);
      }

      if (cached === undefined) {
        cached = kind.call(this);
      }
      return cached;
    },
  });
}

for (const ContainerClass of [
  LoroMap,
  LoroText,
  LoroList,
  LoroTree,
  LoroMovableList,
  LoroCounter,
]) {
  __loroCacheContainerIdGetter(ContainerClass);
  __loroCacheKindMethod(ContainerClass);
}
