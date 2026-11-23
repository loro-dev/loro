# 07. JsonSchema（`docs/JsonSchema.md`）编码：MoonBit 从 JsonSchema 生成 FastUpdates

本文件记录 MoonBit 侧实现 `docs/JsonSchema.md` **编码**（encode / import）的关键约定与实现细节：

- 输入：JsonSchema JSON（字符串）
- 输出：`FastUpdates(mode=4)` 二进制 blob（可被 Rust `LoroDoc.import(...)` 导入）

实现代码：

- `moon/loro_codec/json_schema_import.mbt`（`encode_fast_updates_from_json_schema`）
- CLI：`moon/cmd/loro_codec_cli/main.mbt`（`encode-jsonschema`）

---

## 7.1 API / CLI

MoonBit API：

- `encode_fast_updates_from_json_schema(json: String, validate: Bool) -> Bytes`

CLI：

- `loro-codec encode-jsonschema <in.json> <out.blob>`

---

## 7.2 输入约定：peer 压缩与 ID 解析

JsonSchema root 字段（见 `docs/JsonSchema.md`）：

```ts
{
  schema_version: 1,
  start_version: Record<string, number>,
  peers: string[], // optional
  changes: Change[],
}
```

MoonBit 解析 ID / ContainerID 时有两种模式：

1. **有 peers（peer 压缩）**：`id = "{counter}@{peer_idx}"`，其中 `peer_idx` 是 `0..peers.length-1`；
2. **无 peers（不压缩）**：`id = "{counter}@{peer_id}"`，其中 `peer_id` 直接是 64-bit PeerID 的十进制字符串。

> Rust 的 `LoroDoc.export_json_updates(...)` 默认会输出带 `peers` 的压缩格式，因此主要路径是 (1)。

---

## 7.3 为什么必须校验 counter 连续性

FastUpdates 的二进制 `ChangeBlock` 里并不会为每个 `Op`/`Change` 显式存储完整的 “start counter” 列表。

- 对一个 peer 的 changes：下一条 change 的 start counter 由上一条 change 的 `atom_len(op_len 累加)` 推导；
- 对 change 内的 ops：同理，op 的 counter 序列由 `op.len()` 推导。

因此 JsonSchema → ChangeBlock 时必须确保：

- 同一个 peer 内：按 `change.id.counter` 排序后 **连续**；
- 每个 change 内：按 `op.counter` 排序后 **连续**；
- 并且 `expected += op.len()` / `expected += atom_len` 的推导关系成立。

MoonBit 在 `jsonschema_import_sort_and_validate_changes(...)` 中做了上述验证；不满足时会报错。

---

## 7.4 分块策略：按 peer 编成多个 ChangeBlock

编码流程（简化）：

1. 解析所有 `changes[]` 为 MoonBit 的 `Change`/`Op`；
2. 按 **真实 peer id** 分组；
3. 每个 peer 生成一个 `DecodedChangeBlock`，调用 `encode_change_block(...)` 得到 block bytes；
4. 把所有 blocks 写入 `FastUpdates(mode=4)` body（`ULEB128(len) + bytes` * N）；
5. 用 `encode_document(4, body)` 生成带 checksum 的最终 blob。

`validate=true` 时会对每个生成的 block 再做一次 `decode_change_block(...)` 自校验，提前发现编码错误。

---

## 7.5 Op / Value 支持范围与限制

当前支持的容器类型：

- `Map` / `List` / `Text` / `Tree` / `MovableList` / `Counter`

当前限制：

- `UnknownOp` 暂不支持（遇到会报错）。
- `Counter` 的 JsonSchema 形态使用 `JsonOpContent::Future`（字段 `type="counter"` + `prop` + `value_type/value`），目前仅支持：
  - `prop == 0`
  - `value_type` 为 `f64` 或 `i64`（会编码为二进制 values 段里的 `F64/I64`）
- `LoroValue::Container`（JSON 中 `"🦜:cid:..."`）仅支持 normal container，并且要求它的 `peer/counter` 与当前 op 的 `op_id(peer, counter)` **一致**：
  - 二进制 ValueEncoding 里对 container value 只存 `container_type`（不存 peer/counter），因此必须从 `op_id` 推回 container id；
  - root container value（`cid:root-*`）在二进制 value 里不可表示，目前直接拒绝。
- `LoroValue::Binary` 使用 `"🦜:binary:{Base64}"`（标准 RFC 4648 Base64，校验长度、字符集与 canonical padding bits）还原为 `Binary`；普通 JSON 数组仍解析为 `List`（兼容旧版 JsonSchema 输出）。

---

## 7.6 `start_version` 的处理

JsonSchema 的 `start_version` 在编码为 FastUpdates 时会被 **忽略**：

- FastUpdates 二进制格式不携带 `start_version`
- 导入方（Rust `LoroDoc.import(...)`）也不需要它

如果需要基于 `start_version` 做“补齐缺失历史”的工具链，建议在更外层协议中单独保存它。
