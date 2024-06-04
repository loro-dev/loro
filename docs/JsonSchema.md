# JSON Schema for Loro's OpLog

## Introduction 

Loro supports multiple data structures and introduces many new concepts. Having only binary export formats would make it difficult for developers to understand the underlying processes. Better transparency leads to better developer experience. A human-readable JSON representation enables users to better understand and operate the document and to develop related tools.

To better understand this document, you may first need to understand how Loro stores historical editing data:

- [OpLog](https://www.loro.dev/docs/advanced/doc_state_and_oplog)
- [`Change`, `Operation`](https://www.loro.dev/docs/advanced/op_and_change)
- [`Replayable Event Graph (REG)`](https://www.loro.dev/docs/advanced/replayable_event_graph)

# Specification

## Root object

The root object contains all `Change`s, `Op`s, and critical metadata like start/end versions and schema version.

We will also extract the 64-bit integer PeerID to the beginning of the document and replace it internally with incrementing numbers starting from zero: 0, 1, 2, 3... This significantly reduces the document size and enhances readability.

```ts
{
    "schema_version": number,
    "start_version": Map<string, number>,
    "peers": string[],
    "changes": Change[],
}
```

- `schema_version`: the version of the schema that the document is encoded with. It's 1 for the current specification.
- `start_version`: the start `Frontiers` version of the document. They are represented as a map from the decimal string representation of `PeerID` to `Counter`.
- `peers`: the list of peers in the document. We represent all PeerIDs as decimal strings to avoid exceeding JavaScript's number limit.
- `changes`: the list of changes in the document.

## Changes

`Change`s are crucial in the OpLog. A REG([Replay event graph](https://www.loro.dev/docs/advanced/replayable_event_graph)) is a directed acyclic graph where each node is a `Change`, and each edge is a causal dependency between `Change`s. The metadata of the `Change`s helps us reconstruct the graph.

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

Operation (abbreviated as `Op`) is the most complex part of the document. Loro currently supports multiple containers `List`, `Map`, `RichText`, `Movable List` and `Movable Tree`. Each data structure has several different `Op`s.

But in general, each `Op` is composed of the `ContainerID` of the container that created it, a counter, and the corresponding content of the `Op`.

```ts
type Op = {
    "container": ContainerID,
    "counter": number,
    "content": OpContent // Its detailed definition is elaborated below, with different types for different Containers.
};

type OpContent = ListOp | TextOp | MapOp | TreeOp | MovableListOp | UnknownOp;
type ContainerID =
  | `cid:root-${string}:${ContainerType}`
  | `cid:${number}@${PeerID}:${ContainerType}`;
```

- `container`: the `ContainerID` of the container that created this `Op`, represented by a string starts with `cid:`.
- `counter`: the counter part of the OpID
- `content`: the semantic content of the `Op`, it is different for each field depending on the `Container`.

The following is the **content** of each container。

### List

```ts
type ListOp = ListInsertOp | ListDeleteOp;
```

#### Insert

```ts
type ListInsertOp = {
    "type": "insert",
    "pos": number,
    "value": LoroValue
}
```

- `type`: `insert`.
- `pos`: the index of the insert operation.
- `value`: the insert content which is a list of `LoroValue`

#### Delete

```ts
type ListDeleteOp = {
    "type": "delete",
    "pos": number,
    "len": number,
    "start_id": OpID
}
```

- `type`: `delete`.
- `pos`: the start index of the deletion.
- `len`: the length of deleted content.
- `start_id`: the string id of start element deleted.

### MovableList

```ts
type MovableListOp = ListInsertOp | ListDeleteOp | MovableListMoveOp | MovableListSetOp;
```

#### Insert

```ts
type ListInsertOp = {
    "type": "insert",
    "pos": number,
    "value": LoroValue
}
```

- `type`: `insert`,
- `pos`: the index of the insert operation.
- `value`: the insert content which is a list of `LoroValue`

#### Delete

```ts
type ListDeleteOp = {
    "type": "delete",
    "pos": number,
    "len": number,
    "start_id": OpID
}
```

- `type`: `delete`
- `pos`: the start index of the deletion.
- `len`: the length of deleted content.
- `start_id`: the string id of start element deleted.

#### Move

```ts
type MovableListMoveOp = {
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
- `elem_id`: the ID (described by lamport@peer) of the element moved.

#### Set

```ts
type MovableListSetOp = {
    "type": "set",
    "elem_id": ElemID,
    "value": LoroValue
}

type ElemID = `L${number}@${PeerID}`
```

- `type`:`insert`, `delete`, `move` or `set`.
- `elem_id`: the ID (described by lamport@peer) of the element replaced.
- `value`: the value set.

### Map

```ts
type MapOp = MapInsertOp | MapDeleteOp;
```

#### Insert

```ts
type MapInsertOp = {
    "type": "insert",
    "key": string,
    "value": LoroValue
}
```

- `type`: `insert`.
- `key`: the key of the insertion.
- `value`: the value of the insertion.

#### Delete

```ts
type MapDeleteOp = {
    "type": "delete",
    "key": string
}
```

- `type`: `delete`.
- `key`: the key of the deletion

### Text

```ts
type TextOp = TextInsertOp | TextDeleteOp | TextMarkOp | TextMarkEndOp;
```

#### Insert

```ts
type TextInsertOp = {
    "type": "insert",
    "pos": number,
    "text": string
}
```

`type`: `insert`.
`pos`: the index of the insert operation. The position is based on the Unicode code point length.
`text`: the string of the insertion.

#### Delete

```ts
type TextDeleteOp = {
    "type": "delete",
    "pos": number,
    "len": number,
    "start_id": OpID
}
```

`type`: `delete`.
`pos`: the index of the deletion. The position is based on the Unicode code point length.
`len`: the length of the text deleted.
`start_id`: the string id of the beginning element deleted.


#### Mark

```ts
type TextMarkOp = {
    "type": "mark",
    "start": number,
    "end": number,
    "style_key": string,
    "style_value": LoroValue,
    "info": number
}
```

`type`: `mark`
`start`: the start index of text need to mark. The position is based on the Unicode code point length.
`end`: the end index of text need to mark. The position is based on the Unicode code point length.
`style_key`: the key of style, it is customizable.
`style_value`: the value of style, it is customizable.
`info`: the config of the style, whether to expand the style when inserting new text around it.

#### MarkEnd

```ts
type TextMarkEndOp = {
    "type": "mark_end"
}
```

`type`: `mark_end`.

### Tree

```ts
type TreeOp = TreeCreateOp | TreeMoveOp | TreeDeleteOp;
```

#### Create

```ts
type TreeCreateOp = {
    "type": "create",
    "target": TreeID,
    "parent": TreeID | null,
    "fractional_index": string
}

type TreeID = `${number}@${PeerID}`
```

- `type`: `create`.
- `target`: the string format of target `TreeID` moved.
- `parent`: the string format of `TreeID` or `null`. If it is `null`, the target node will be a root node.
- `fractional_index`: the fractional index with hex string format of the target node.

#### Move

```ts
type TreeMoveOp = {
    "type": "move",
    "target": TreeID,
    "parent": TreeID | null,
    "fractional_index": string
}

type TreeID = `${number}@${PeerID}`
```

- `type`: `move`.
- `target`: the string format of target `TreeID` moved.
- `parent`: the string format of `TreeID` or `null`. If it is `null`, the target node will be a root node.
- `fractional_index`: the fractional index with hex string format of the target node.

#### Delete

```ts
type TreeDeleteOp = {
    "type": "delete",
    "target": TreeID
}

type TreeID = `${number}@${PeerID}`
```

- `type`: `delete`.
- `target`: the string format of target `TreeID` deleted.

### Unknown

To support forward compatibility, we have an unknown type. When an `Op` with a newly supported Container from a newer version is decoded into the older version, it will be treated as an unknown type in a more general form, such as binary and string. When the new version decodes an unknown `Op`, the newer version of Loro will know its true type and decode correctly.

```ts
type UnknownOp = {
    "type": "unknown",
    "prop": number,
    "value_type": string,
    "value": `${EncodeValue}`
}
```

- `type`: just an unknown type.
- `prop`: a property of the encoded op, it's a number.
- `value_type`: the type of `EncodeValue`.
- `value`: common data types used in encoding with json string format.

## Value

In this section, we will introduction two *Value* in Loro. One is `LoroValue`, it's an enum of data types supported by Loro, such as the value inserted by `List` or `Map`.

The another is `EncodedValue`, it's just used in encoding module for unknown type.

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

### EncodedValue

The `EncodedValue` is the specific type used by Loro when encoding, it's an internal value, users do not need to get it clear. It is specially designed to handle the schema mismatch due to forward and backward compatibility. In JSON encoding schema, the `EncodedValue` will be encoded as an object.
