# Json Representation of Loro

## Motivation

Loro is diligently going towards the v1.0 stable version, during which the encoding format of Loro will inevitably bring some changes. In order to allow early trial users to smoothly migrate their historical data, Loro needs to add a more universal, and even human-readable, self-describing encoding format. This can also serve as the data source for future Loro dev-tools for visualization.

The following is a introduction of this specification.

## Document

The document is the highest level of the specification. It consist of all `Changes` and `Operations` and some metadata that describes the document such as the start/end version, the schema version, etc. The global context will also be at this level. the role of the global context is to reduce the document's encoding size and enhance its readability. For example, a `PeerID` of `u64` not only interferes with vision but is also completely unhelpful in distinguishing multiple peers. It is more intuitive to use simple numbers such as the index 0, 1, and 2.

```ts
{
    "schema_version": number,
    "start_version": Record<string, number>,
    "end_version": Record<string, number>,
    "peers": string[],
    "changes": Change[],
}
```

- `schema_version`: the version of the schema that the document is encoded with. In case for need, we can add a new schema and decode the document with the old schema.
- `start_version` and `end_version`: the start and end version of the document. They are represented as a map from the decimal string representation of `PeerID` to `Counter`.
- `peers`: the list of peers in the document. Here we use a decimal string to represent.
- `changes`: the list of changes in the document.

## Changes

The Change is an important part of the document. A REG([Replay event graph](https://www.loro.dev/docs/advanced/replayable_event_graph)) is a directed graph where each node is a `Change` and each edge is a causal dependency between `Changes`. Except for the first Change of each peer, all Changes have one or more causal dependencies, we can use all the `Change` information to reconstruct the event graph between the start and end versions of the document.

At the same time, a `Change` represents a transaction of a document. If you are familiar with VCS (version control systems) like [git](https://git-scm.com/), this is also equivalent to a commit. You can attach a commit message to each `Change` to describe the effect of this `Change`, that is the effect of all `Operation`s in `Change`.

```ts
{
    "id": string,
    "timestamp": number,
    "deps": string[],
    "lamport": number,
    "msg": string,
    "ops": Op[]
}
```

- `id`: the string representation of the unique `ID` of each `Change`, in the form of `{Counter}@{PeerID}` which is the `@` character connecting `Counter` and `PeerID`. Of course, This `PeerID` is the index of peers in the global context.
- `timestamp`: the number of Unix timestamp when the change is committed.
- `deps`: a list of causal dependency of this `Change`, each item is the `ID` represented by a string.
- `lamport`: the lamport timestamp of the `Change`.
- `msg`: the commit message.
- `ops`: all of the `Op` in the `Change`.

## Operations

Operation (abbreviated as `Op`) is the most complex part of the document. Loro currently supports multiple containers `List`, `Map`, `RichText`, `Movable List` and `Movable Tree`. Each data structure has several different semantic `Op`s.

But in general, each `Op` is composed of the `ContainerID` of the container that created it, a counter, and the corresponding content of the `Op`.

```ts
{
    "container": string,
    "counter": number,
    "content": object
}
```

- `container`: the `ContainerID` of the container that created this `Op`, represented by a string starts with `cid:`.
- `counter`: a part of lamport timestamp, when equal, `PeerID` is used as the tie-breaker.
- `content`: the semantic content of the `Op`, it is different for each field depending on the `Container`.

The following is the **content** of each container。

### List

#### Insert

```ts
{
    "type": "insert",
    "pos": number,
    "value": LoroValue
}
```

- `type`: `insert` or `delete`.
- `pos`: the index of the insert operation.
- `value`: the insert content which is a list of `LoroValue`

#### Delete

```ts
{
    "type": "delete",
    "pos": number,
    "len": number,
    "delete_start_id": string
}
```

- `type`: `insert` or `delete`.
- `pos`: the start index of the deletion.
- `len`: the length of deleted content.
- `delete_start_id`: the string id of start element.

### MovableList

#### Insert

```ts
{
    "type": "insert",
    "pos": number,
    "value": LoroValue
}
```

- `type`: `insert`, `delete`, `move` or `set`.
- `pos`: the index of the insert operation.
- `value`: the insert content which is a list of `LoroValue`

#### Delete

```ts
{
    "type": "delete",
    "pos": number,
    "len": number,
    "delete_start_id": string
}
```

- `type`:`insert`, `delete`, `move` or `set`.
- `pos`: the start index of the deletion.
- `len`: the length of deleted content.
- `delete_start_id`: the string id of start element.

#### Move

```ts
{
    "type": "move",
    "from": number,
    "to": number,
    "from_id": string
}
```

- `type`:`insert`, `delete`, `move` or `set`.
- `from`: the index of the element before is moved.
- `to`: the index of the index moved to after moving out the element
- `from_id`: the string id of the element moved.

#### Set

```ts
{
    "type": "set",
    "elem_id": string,
    "value": LoroValue
}
```

- `type`:`insert`, `delete`, `move` or `set`.
- `elem_id`: the string id of the element replaced.
- `value`: the value setted.

### Map

#### Insert

```ts
{
    "type": "insert",
    "key": string,
    "value": LoroValue
}
```

- `type`: `insert` or `delete`.
- `key`: the key of the insertion.
- `value`: the value of the insertion.

#### Delete

```ts
{
    "type": "delete",
    "key": string
}
```

- `type`: `insert` or `delete`.
- `key`: the key of the deletion

### Text

#### Insert

```ts
{
    "type": "insert",
    "pos": number,
    "text": string
}
```

#### Delete

```ts
{
    "type": "delete",
    "pos": number,
    "len": number,
    "id_start": string
}
```

#### Mark

```ts
{
    "type": "mark",
    "start": number,
    "end": number,
    "style_key": string,
    "style_value": LoroValue,
    "info": number
}
```

#### MarkEnd

```ts
{
    "type": "mark_end"
}
```

### Tree

#### Move

```ts
{
    "type": "move",
    "target": string,
    "parent": string | null,
    "fractional_index": UInt8Array
}
```

- `type`: `move` or `delete`.
- `target`: the string format of target `TreeID` moved.
- `parent`: the string format of `TreeID` or `null`. If it is `null`, the target node will be a root node.
- `fractional_index`: the fractional index of the target node.

#### Delete

```ts
{
    "type": "delete",
    "target": string
}
```

- `type`: `move` or `delete`.
- `target`: the string format of target `TreeID` deleted.

### Unknown

To support backward and forward compatibility of encoding, we have an unknonw type. When an `Op` from newer version is decoded into the older version, it will be treated as an unknown type which is in a more general form, such as binary and string. Conversely, when the new version decodes an unknown `Op`, Loro will know its true type and perform the proper decoding process.

So there are two kind of unknown format, binary format and json-string format.

#### Binary Unknown

```ts
{
    "type": "unknown",
    "prop": number,
    "value_type:": "unknown",
    "value": OwnedValue
}
```

- `type`: just an unknown type.
- `prop`: a property of the encoded op, it's a number.
- `value_type`: unknown.
- `value`: 

#### Json Unknown

```ts
{
    "type": "unknown",
    "prop": number,
    "value_type": "json_unknown",
    "value": string
}
```

- `type`: just an unknown type.
- `prop`: a property of the encoded op, it's a number.
- `value_type`: json_unknown.
- `value`: a string json format of `OwnedEncodeValue`

## Value

In this section, we will introduction two *Value* in Loro. One is `LoroValue`, it's an enum of data types supported by Loro, such as the value inserted by `List` or `Map`. 

The another is `OwnedEnodeValue`, it's just used in encoding module for unknown type.


### LoroValue

These are data types supported by Loro and its json format:

- `null`: `null` or `undefined`
- `Bool`: `true` or `false`
- `F64`: `number`(float)
- `I64`: `number`(signed)
- `Binary`: `UInt8Array`
- `String`: `string`
- `List`: `Array<LoroValue>`
- `Map`: `Map<string, LoroValue>`
- `Container`: the id of container. `🦜:cid:{Counter}@{PeerID}:{ContainerType}` or `🦜:cid:root-{Name}:{ContainerType}`

Note: Compared with the string format, we add a prefix `🦜:` when encoding the json format of `ContainerID` to prevent users from saving the string format of `ContainerID` and misinterpreting it as `ContainerID` when decoding.

### OwnedEncodeValue

