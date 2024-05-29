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

Operation (abbreviated as Op) 