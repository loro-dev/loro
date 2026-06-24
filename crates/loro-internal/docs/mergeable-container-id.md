# Mergeable Container ID Encoding

Mergeable child containers are represented as synthetic `ContainerID::Root`
values. The root name is internal and starts with the reserved namespace prefix:

```text
🤝:<payload>
```

The child container type is not encoded in `<payload>`. It is carried by
`ContainerID::Root.container_type`, exactly like ordinary root containers.

## Payload

`<payload>` is a flattened map path:

```text
payload = base-parent ">" key-1 ">" key-2 ...
```

There is no version byte in this format. It replaces the previous unpublished
encoding.

`<base-parent>` is the nearest non-mergeable map ancestor:

```text
$<escaped-root-name>
@<peer-base36>:<counter-base36>
```

`$` means the base parent is a root map. The rest of the segment is the escaped
root name. `@` means the base parent is a normal op-created map. The peer id and
counter are canonical lowercase base36 encoded to keep the payload compact.
Leading zeroes, uppercase digits, and `-0` are rejected by the parser because
`new_mergeable` never emits them.

Every following segment is one escaped map key. Intermediate mergeable parents
are always maps, so their type is omitted. The final container's type is the
`Root.container_type` field.

For example:

```text
Root map "state", key "note-1", child map:
🤝:$state>note-1        with Root.container_type = Map

Nested key "body" under that mergeable map, child text:
🤝:$state>note-1>body   with Root.container_type = Text
```

Parsing `🤝:$state>note-1>body` as a `Text` returns:

```text
parent = Root("🤝:$state>note-1", Map)
key = "body"
container_type = Text
```

This keeps nesting growth linear in the total path length. It does not embed the
full serialized parent cid at each level.

## Escaping

Segments are escaped before being placed in the synthetic root name:

```text
\  -> \\
>  -> \>
/  -> \s
NUL -> \0
```

`>` is the only structural delimiter. `\` introduces an escape. `/` and NUL are
also escaped so synthetic root names keep the same safety property as the old
hex encoding: they do not contain raw slash or raw NUL bytes.

The parser rejects dangling backslashes, unknown escapes, raw slash, and raw NUL.
This prevents malformed synthetic roots from being treated as mergeable ids.

## Marker Relationship

The parent map slot still stores the binary mergeable marker. The marker binds
`(parent container id, key, child type)` through its digest and is not changed by
this encoding.

The root name encoding only controls the deterministic identity of the child
container. Visibility remains controlled by the parent map slot marker.
