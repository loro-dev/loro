# `doc.toJsonWithReplacer`

`toJsonWithReplacer` is a utility that lets you turn the content of a `LoroDoc` into plain JSON **while still giving you full control over how every field and every container is represented**.  
It follows the familiar semantics of the second argument of `JSON.stringify(value, replacer)` but understands Loro specific data-structures such as `LoroText`, `LoroMap`, `LoroList`, `LoroTree`, ….

```ts
const json = doc.toJsonWithReplacer(
  (key, value) => /* your custom transformation  */
);
```

## Why would I want this?

*   Include extra information (for example the internal container id) next to the data so that the receiver can later map the JSON back to Loro containers.
*   Convert rich-text (`LoroText`) to Delta, markdown, plain text …
*   Filter out confidential sub trees before sending the data over the wire.
*   Replace large binary blobs with references.

## The replacer callback

```
(key: string | number, value: Value | Container) => Value | Container | undefined
```

* **key** – property name (or index for array elements). For the root invocation `key` will be an empty string.
* **value** – either a JSON value (`number`, `string`, `boolean`, `null`, `object`, `array`) **or** a Loro *container* (`LoroText`, `LoroMap`, …).
* **return value** –
  *   `undefined` – the key will be **omitted** from the final JSON (just like in `JSON.stringify`).
  *   A primitive value / plain object / array – the returned value is used **as is**.  If a plain object or array is returned, **`toJsonWithReplacer` will recursively walk it**, replacing any embedded container ids it finds (see "Walking child containers" below).
  *   **The **same** container object** that was supplied in `value` – this signals that you accept the container unchanged.  In that case `toJsonWithReplacer` internally calls `container.getShallowValue()` and keeps traversing the result so that **all nested containers are still visited**.

🚫  **You must not** return any *other* container instance (newly constructed or a different one) – this would break the internal dependency graph and therefore throws.

## Including the container id

Every container instance exposes an `id` string that uniquely identifies it inside the document.  A common pattern is to enrich the serialized JSON with that id so that the consumer can later re-establish references back to the original CRDT object:

```ts
const json = doc.toJsonWithReplacer((key, value) => {
  if (isContainer(value)) {
    return {
      id: value.id,              // 👈 container id
      value: value.getShallowValue(),   // the container's own data (still walked recursively)
    };
  }
  return value;
});
```

The snippet above produces output of the form

```json
{
  "users": {
    "id": "cid:root-users:List",
    "value": [
      { "name": "Alice" },
      { "name": "Bob" }
    ]
  }
}
```

## Walking child containers

`toJsonWithReplacer` looks for **container id strings** (`"cid:<…>"`) inside every object or array that is returned from the replacer **and that it did not skip**.  For each id that it encounters **exactly once** it will:

1. Resolve the id to the actual container instance (`doc.getContainerById(id)`).
2. Call your replacer callback with that container so you can decide how it should be serialized.
3. Use the value you returned and – if it is an object / array or if you returned the container itself – keep walking it recursively.

Because each container id is processed at most once `toJsonWithReplacer` is safe against cyclic references.

### Example – selectively traversing children

```ts
const json = doc.toJsonWithReplacer((key, value) => {
  // Replace lists with an object that contains the id AND the raw list elements
  if (value instanceof LoroList) {
    return { id: value.id, items: value.getShallowValue() };
  }
  // Everything else unchanged
  return value;
});
```

Even though we replaced a list with a plain object, `items` still contains **container id strings** for any nested maps/texts.  `toJsonWithReplacer` therefore continues walking and will invoke your replacer for those children as well.  If you *don't* want that behaviour simply strip the container ids in your own return value.

## Tips & foot-guns

*   The callback is executed **depth-first**.
*   It is fine to call container helper methods like `toDelta()`, `toJSON()` or `getShallowValue()` inside the replacer – they do **not** mutate the document.
*   Returning a new container or mutating a container inside the replacer throws – the method is meant to be side-effect free.
*   If you only need a quick *plain* JSON representation of the document you can pass the identity function: `doc.toJsonWithReplacer((_k,v)=>v)`.