# 16. Import / Export Updates Spec（FastUpdates mode=4）

本文件定义 MoonBit LoroDoc runtime 的 import/export 语义与与 `moon/loro_codec` 的对接方式，目标是与 Rust/TS 的 updates（FastUpdates，mode=4）互通。

> 真值来源：
> - Rust export updates：`crates/loro-internal/src/encoding/fast_snapshot.rs::encode_updates`
> - Rust decode updates：`crates/loro-internal/src/encoding/fast_snapshot.rs::decode_updates`
> - Moon codec：`moon/loro_codec/document.mbt` + `change_block_decode.mbt` + `change_block_encode.mbt`

---

## 16.1 支持范围

必须支持：

- `EncodeMode = 4 (FastUpdates)`：import/export

可选（后续）：

- `EncodeMode = 3 (FastSnapshot)`：import snapshot / export snapshot
- ShallowSnapshot / StateOnly / SnapshotAt 等高级导出模式

明确不支持：

- outdated encodings（mode=1/2）

---

## 16.2 Updates blob 结构（mode=4）

FastUpdates body 为一系列 ChangeBlock bytes，每个 block 以 ULEB128 长度前缀编码：

```
body := (uleb128(len) + block_bytes[len])*
```

每个 block 是单 peer 的 ChangeBlock（header.peers[0] 为该 peer）。

Moon codec 真值参考：

- 解析：`moon/loro_codec/document.mbt::parse_fast_updates_body`
- block 解码：`moon/loro_codec/change_block_decode.mbt::decode_change_block`

---

## 16.3 Import 语义（bytes → oplog/state）

### 16.3.1 解析流程

1. `parse_document(bytes, validate=true)` 校验 magic/mode/checksum
2. 若 mode!=4：报错（ImportUnsupportedEncodingMode）
3. `parse_fast_updates_body(body)` 得到 `blocks : Array[BytesView]`
4. 对每个 block：
   - `decode_change_block(block)` → `changes : Array[Change]`（IR，见 `moon/loro_codec/change.mbt`）
5. 将所有 changes 扁平化后按确定性顺序处理（见 16.3.2）

### 16.3.2 应用顺序（规范）

Rust 解码后做了 `sort_unstable_by_key(|c| c.lamport)`；为保证 MoonBit 侧确定性，规范建议：

- 按 `(change.lamport, change.id.peer, change.id.counter)` 升序排序

理由：

- lamport 提供因果约束（new change 的 lamport > deps 的 lamport）
- peer/counter tie-break 保证同 lamport 并发时顺序稳定（不依赖 sort 的不稳定行为）

### 16.3.3 deps 满足与 pending

对于一个 change，其 deps（frontiers）必须在本地 oplog 中已存在才可应用：

- `deps_satisfied(change)` 当且仅当：对每个 `dep`，`oplog_vv.includes_id(dep)` 为 true

若不满足：

- 将该 change 放入 pending（按 dep 依赖组织或简单队列均可）
- import 返回 `ImportStatus { success, pending }`

当后续 import 带来更多 changes 后，需要重复尝试把 pending 中 deps 已满足的项应用。

> MVP 可先实现“若存在不满足 deps 的 change 直接返回 pending，不应用后续”，但最终需要收敛到可重试的 pending 机制。

Rust 真值：`crates/loro-internal/src/encoding.rs` 的导入路径会返回 `ImportStatus { success, pending }`。

### 16.3.4 应用到 state 的原则

规范要求 runtime **不依赖 diff calculator** 也能收敛到与 Rust 相同的最终状态。推荐策略：

- 以 changes/ops 的 IdFull 顺序（lamport+peer+counter）将操作喂给各容器的 CRDT 状态机：
  - Map：LWW by IdLp
  - List/Text：seq-crdt（12）+ 上层语义（14）
  - MovableList：seq-crdt（list-items）+ element LWW（13）
  - Tree：last-move-wins + cycle check（15）

即使后续实现 diff/checkout，也必须保证“直接 apply ops”与“checkout diff”语义一致。

---

## 16.4 Export 语义（oplog → bytes）

### 16.4.1 export({mode:"update", from?})

输入：

- `from : VersionVector`（可选；缺省视为 empty vv）

语义：

- 导出所有 `oplog` 中 **不被 from vv 覆盖** 的 changes（即“增量 updates”）

形式化：

- 对每个 peer：
  - `from_counter = from.get(peer).unwrap_or(0)`
  - 导出该 peer 下所有 `change`，满足 `change.ctr_end() > from_counter`

Rust 真值：`fast_snapshot::encode_updates(doc, vv)` 最终调用 `oplog.export_blocks_from(vv)`。

### 16.4.2 block 组装（MoonBit 侧的可行最小方案）

Moon codec 的 `encode_change_block` 需要 `DecodedChangeBlock`（含 peers/keys/cids/positions 表 + changes）。

最小可行方案（允许与 Rust block 切分不同）：

- 对每个 peer，把需要导出的 changes 按 `change.id.counter` 升序排列
- 以“每个 peer 一个 block”打包：
  - `DecodedChangeBlock.peers = [peer]`
  - `keys/cids/positions = []`（空表；encode 过程中会按需注册）
  - `changes = selected_changes_for_peer`
  - 调用 `encode_change_block(block)` 得到 bytes
- 输出 blocks 的顺序应确定（建议按 peer 升序）

Moon 真值参考：`moon/loro_codec/change_block_encode.mbt`（会从 changes 推导 counter/lamport range）。

### 16.4.3 编码为 Document（mode=4）

- `body = encode_fast_updates_body(block_bytes[])`
- `doc = encode_document(mode=4, body)`

Moon 真值参考：`moon/loro_codec/document.mbt::{encode_fast_updates_body, encode_document}`。

---

## 16.5 互操作验收（与 Rust 对照）

必须满足：

1. Rust `export(update)` → Moon `import` → Moon `toJSON` 与 Rust `get_deep_value` 一致
2. Moon 本地编辑 → Moon `export(update)` → Rust `import` → Rust deep value 与 Moon 一致

测试计划见 `moon/specs/17-lorodoc-test-plan.md`。

