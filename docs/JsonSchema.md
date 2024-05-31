# JSON Representation of Loro's OpLog

## Motivation

Loro supports many data structure semantics and abstracts some concepts such as [OpLog](https://www.loro.dev/docs/advanced/doc_state_and_oplog), [`Change`, `Operation`](https://www.loro.dev/docs/advanced/op_and_change) or [`REG`](https://www.loro.dev/docs/advanced/replayable_event_graph), etc. We hope to provide a more universal, human-readable and self-describing encoding format than the binary encoding format to better help users understand, lookup and modify Loro documents. This can also serve as the data source for future Loro dev-tools for visualization.


## Document

`Document` is the highest level of the specification. It consists of all `Change`s and `Op`s and some metadata that describes the document, such as the start/end version, the schema version, etc. 

We will also extract the 64-bit integer PeerID to the beginning of the document and replace it internally with incrementing numbers starting from zero: 0, 1, 2, 3... This significantly reduces the document size and enhances readability.

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

The Change is an important part of the document. A REG([Replay event graph](https://www.loro.dev/docs/advanced/replayable_event_graph)) is a directed acyclic graph where each node is a `Change`, and each edge is a causal dependency between `Change`s. The metadata of the `Change`s helps us reconstruct the graph.

You can also attach a commit message to a `Change` like you usually do with Git's commit.

```ts
{
    "id": string,
    "timestamp": number,
    "deps": OpID[],
    "lamport": number,
    "msg": string,
    "ops": Op[]
}

type OpID = `${number}@${PeerID}`;
```

- `id`: the string representation of the unique `ID` of each `Change`, in the form of `{Counter}@{PeerID}` which is the `@` character connecting `Counter` and `PeerID`. Of course, This `PeerID` is the index of peers in the global context.
- `timestamp`: the number of Unix timestamp when the change is committed. [Timestamp is not recorded by default](https://loro.dev/docs/advanced/timestamp)
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
    "start_id": OpID
}
```

- `type`: `insert` or `delete`.
- `pos`: the start index of the deletion.
- `len`: the length of deleted content.
- `start_id`: the string id of start element deleted.

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
    "start_id": OpID
}
```

- `type`:`insert`, `delete`, `move` or `set`.
- `pos`: the start index of the deletion.
- `len`: the length of deleted content.
- `start_id`: the string id of start element deleted.

#### Move

```ts
{
    "type": "move",
    "from": number,
    "to": number,
    "elem_id": ElemID
}

type ElemID = `L${number}@${PeerID}`
```

- `type`:`insert`, `delete`, `move` or `set`.
- `from`: the index of the element before is moved.
- `to`: the index of the index moved to after moving out the element
- `elem_id`: the string id of the element moved.

#### Set

```ts
{
    "type": "set",
    "elem_id": ElemID,
    "value": LoroValue
}

type ElemID = `L${number}@${PeerID}`
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

`type`: `insert`, `delete`, `mark` or `mark_end`.
`pos`: the index of the insert operation. The position is based on the Unicode code point length.
`text`: the string of the insertion.

#### Delete

```ts
{
    "type": "delete",
    "pos": number,
    "len": number,
    "id_start": OpID
}
```

`type`: `insert`, `delete`, `mark` or `mark_end`.
`pos`: the index of the deletion. The position is based on the Unicode code point length.
`len`: the length of the text deleted.
`id_start`: the string id of the beginning element deleted.


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

`type`: `insert`, `delete`, `mark` or `mark_end`.
`start`: the start index of text need to mark. The position is based on the Unicode code point length.
`end`: the end index of text need to mark. The position is based on the Unicode code point length.
`style_key`: the key of style, it is customizable.
`style_value`: the value of style, it is customizable.
`info`: the config of the style, whether to expand the style when inserting new text around it.

#### MarkEnd

```ts
{
    "type": "mark_end"
}
```

`type`: `insert`, `delete`, `mark` or `mark_end`.

### Tree

#### Create

```ts
{
    "type": "create",
    "target": string,
    "parent": string | null,
    "fractional_index": UInt8Array
}
```

- `type`: `create`, `move` or `delete`.
- `target`: the string format of target `TreeID` moved.
- `parent`: the string format of `TreeID` or `null`. If it is `null`, the target node will be a root node.
- `fractional_index`: the fractional index of the target node.

#### Move

```ts
{
    "type": "move",
    "target": string,
    "parent": string | null,
    "fractional_index": UInt8Array
}
```

- `type`: `create`, `move` or `delete`.
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

- `type`: `create`, `move` or `delete`.
- `target`: the string format of target `TreeID` deleted.

## Value

In this section, we will introduction two *Value* in Loro. One is `LoroValue`, it's an enum of data types supported by Loro, such as the value inserted by `List` or `Map`.

The another is `EncodeValue`, it's just used in encoding module for unknown type.

### LoroValue

These are data types supported by Loro and its json format:

- `null`: `null`
- `Bool`: `true` or `false`
- `F64`: `number`(float)
- `I64`: `number` or `bigint` (signed)
- `Binary`: `UInt8Array`
- `String`: `string`
- `List`: `Array<LoroValue>`
- `Map`: `Map<string, LoroValue>`
- `Container`: the id of container. `🦜:cid:{Counter}@{PeerID}:{ContainerType}` or `🦜:cid:root-{Name}:{ContainerType}`

Note: Compared with the string format, we add a prefix `🦜:` when encoding the json format of `ContainerID` to prevent users from saving the string format of `ContainerID` and misinterpreting it as `ContainerID` when decoding.
