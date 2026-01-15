# 06. JsonSchema（`docs/JsonSchema.md`）导出：MoonBit 实现约定与细节

本文件记录 MoonBit 侧实现 `docs/JsonSchema.md` **导出**（export）的关键约定与实现细节，方便后续扩展测试与排查差异。

实现代码：

- `moon/loro_codec/json_schema.mbt`
- CLI：`moon/cmd/loro_codec_cli/main.mbt`（`export-jsonschema`）

> 注意：本文只讨论 **导出**（FastUpdates 二进制 → JsonSchema JSON）。JsonSchema → FastUpdates 的 **编码**细节见 `moon/specs/07-jsonschema-encode.md`。

---

## 6.1 总体结构与 peer 压缩

JsonSchema 根对象：

```ts
{
  schema_version: 1,
  start_version: Record<string, number>,
  peers: string[],
  changes: Change[],
}
```

与 `docs/JsonSchema.md` 一致：

- `peers` 存放 **真实 PeerID(u64)** 的十进制字符串（避免 JS number 精度问题）。
- `Change.id` / `Change.deps` / `TreeID` / `ElemID` / `ContainerID(normal)` 中的 `{PeerID}` 都是 **peer index**（`0..peers.length-1`），即“peer 压缩”后的编号。

MoonBit 侧做法：

- 扫描 Change/Op 时动态 `register_peer(actual_peer_id)`，把它分配到 `peer_idx`，并把 `actual_peer_id` 追加到 `peers[]`。
- 导出 `id` 字段时使用 `{counter}@{peer_idx}`。

---

## 6.2 `start_version` 的重建策略（从二进制 FastUpdates 推导）

Rust 的 `export_json_updates(start_vv, end_vv)` 会在 JSON 中携带 `start_version = vv_to_frontiers(start_vv)`。

但 **FastUpdates 二进制格式本身不显式携带 start_vv**，所以 MoonBit 导出函数
`export_json_schema_from_fast_updates(bytes, validate)` 采用“best-effort”推导：

1. 先解出本次 blob 内包含的 change 集合 `included_ids`。
2. 遍历每个 change 的 deps：
   - 若 dep 不在 `included_ids` 中，则认为它属于“导出范围外的依赖”（external dep）
3. 对每个真实 peer，取 external deps 的最大 counter，作为 `start_version[peer]` 的值。

该推导在典型场景下可得到与 Rust `start_version` 一致的结果：

- `all_updates()`：通常 external deps 为空 ⇒ `start_version = {}`
- `Updates { from: vv_v1 }`：external deps 通常包含 `vv_v1` 的 frontier ⇒ `start_version` 非空

> 备注：Rust 侧导入 json updates 目前不会使用 `start_version` 做硬校验，但它对 debug / tooling 很有价值，所以仍然尽量对齐 Rust。

---

## 6.3 数字编码与精度

JsonSchema 的字段里包含 `timestamp(i64)` / `lamport(u32)` / `counter(i32)` 等数值。

MoonBit 输出 JSON 时：

- 仍使用 JSON number 类型
- 但对整型会同时设置 `Json::number(number, repr=...)`，用十进制字符串作为 `repr`

目的：

- JSON 文本层面保留精确整型表示（避免中间链路把大整数变成科学计数法或丢精度）
- Rust `serde_json` 解析依旧以 `repr` 为准，不影响 `loro::JsonSchema` 反序列化

---

## 6.4 `LoroValue::Container` 字符串前缀与 ID 递增规则

`docs/JsonSchema.md` 规定：当 `LoroValue` 是 Container 时，在 JSON 中编码为：

```
"🦜:cid:{Counter}@{PeerID}:{ContainerType}"
```

其中 `{PeerID}` 同样是 **peer index**。

MoonBit 侧目前只在二进制 `ValueEncoding` 里拿到 `ContainerType`（对应 `LoroValue::ContainerType`），需要结合 **当前 op 的 ID** 构造 ContainerID：

- `ContainerID` 使用 `op_id = ID(change_peer, op.counter)` 作为 `{Counter}@{PeerID}` 的基础
- 对 `ListInsertOp.value`（数组）按 Rust 的规则做 `id.inc(i)`：
  - 第 `i` 个元素使用 `ID(change_peer, op.counter + i)`
- 对 `MapInsertOp.value`（map value）使用同一个 `op_id`（不递增）

---

## 6.5 当前限制

- 仅支持从 `FastUpdates(mode=4)` 二进制导出 JsonSchema（不支持 FastSnapshot）。
- `UnknownOp` 目前输出为占位结构（`value_type="unknown", value=null`），用于保持导出可用；后续如需要可对齐 Rust 的 `OwnedValue` / `EncodedValue` 细节。
