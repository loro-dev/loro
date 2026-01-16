# 14. RichText Spec（Text + Styles）

本文件整理 Loro 的 RichText（`LoroText`）语义：索引体系、插入/删除、样式 mark（StyleStart/End）、expand 规则，以及 `toDelta/toString/toJSON` 的输出约定。

> 真值来源：
> - RichText index 说明与 ExpandType：`crates/loro-internal/src/container/richtext.rs`
> - Style config：`crates/loro-internal/src/container/richtext/config.rs`
> - RichtextState/Chunk/坐标换算：`crates/loro-internal/src/container/richtext/richtext_state.rs`
> - State 层（apply_local_op 等）：`crates/loro-internal/src/state/richtext_state.rs`
> - TS/WASM API：`crates/loro-wasm/src/lib.rs::LoroText`

---

## 14.1 索引体系（必须明确）

Loro RichText 同时存在多种 index：

1. **Unicode index**：按 unicode scalar（Rust `chars()`）计数的字符索引（用户 API 默认）
2. **Utf16 index**：按 UTF-16 code unit 计数（JS 常用；WASM API 可提供）
3. **Entity index**：unicode 字符 + style anchors 的混合序列索引
4. **Event index**：内部用于事件/增量（delta）计算的索引（实现细节；可先不对外暴露）

规范要求：

- **持久化到 oplog 的 text ops 使用 Entity index**（而不是 Unicode index）
- 用户 API 接受 Unicode index（或可选 Utf16），在内部转换为 Entity index

Rust 真值：`crates/loro-internal/src/container/richtext.rs` 文件头注释。

---

## 14.2 CRDT 基础：seq-crdt + chunk 类型

RichText 的“序列 CRDT”由 12-seq-crdt 提供，区别在于 chunk 类型：

- TextChunk：表示一段文本（UTF-8 bytes slice + unicode_len + utf16_len + id）
- StyleAnchor：表示样式锚点（Start/End），每个锚点占 1 个 entity
- Unknown/MoveAnchor：用于 GC/checkout/move 等（MVP 可先不实现完整）

Rust 真值：

- chunk 定义：`crates/loro-internal/src/container/richtext/fugue_span.rs::RichtextChunk`
- text chunk：`crates/loro-internal/src/container/richtext/richtext_state.rs::TextChunk`

---

## 14.3 文本操作（insert/delete）

### 14.3.1 insert(index,text)

用户 API：

- `insert(unicode_index, text)`：在 unicode_index 处插入字符串

内部流程（规范）：

1. 把 `unicode_index` 转换为对应的 **Entity index + Side**（见 14.6 expand 规则）
2. 产生一个插入 op（len = `text.chars().count()`）
3. 在 seq-crdt 中插入对应长度的 text chunk

注意：

- `len` 必须按 unicode scalar count 计算（不能用 UTF-16）

Rust 真值：`unicode_to_utf8_index` 等换算函数（`richtext_state.rs`）。

### 14.3.2 delete(index,len)

用户 API：

- `delete(unicode_index, len_unicode)`

内部流程：

1. 转换到 entity index 范围（unicode→entity）
2. 产生 DeleteSpanWithId（支持 reverse；MVP 可先只生成 forward）
3. 在 seq-crdt 上执行 delete（tombstone）

---

## 14.4 样式操作（mark / unmark）

### 14.4.1 StyleStart / StyleEnd 的持久化形态

Loro 用两个 entity 表示一段样式：

- **Start anchor**（StyleStart）：携带 `key/value/info` 以及 `len=end-start`
- **End anchor**（StyleEnd / MarkEnd）：仅占用一个 op_id 位置，用于配对

重要不变量：

- StyleStart 与 StyleEnd 必须配对，且**在 oplog 中相邻**（Rust 约束：StyleStart 的下一个 op 必须是 StyleEnd）
- StyleStart 的 `atom_len` 固定为 1（与其 range.len 无关）

Rust 真值：

- `InnerListOp::StyleStart/StyleEnd`：`crates/loro-internal/src/container/list/list_op.rs`
- 编码值：Value::MarkStart + Null（见 `moon/loro_codec/value_custom_types.mbt` 与 decode_op_content）

### 14.4.2 mark(range,key,value)

输入：

- `range = { start, end }`（unicode index，`start < end`）
- `key : String`（不允许包含 `:`，见 style config）
- `value : Value`（JSON-like）

输出到 oplog 的语义 op：

1. `StyleStart { start:entity_start, end:entity_end, key, value, info }`
2. `StyleEnd`

其中：

- `entity_start/entity_end` 是将 unicode range 映射到 entity index 后的边界
- `info` 来自 doc 的 style config（ExpandType 编码到 TextStyleInfoFlag）

Rust 真值：`StyleConfigMap::get_style_flag` + `TextStyleInfoFlag::new`。

### 14.4.3 unmark(range,key)

unmark 不是“删除两条历史 op”，而是写入新的样式操作以抵消：

1. 读取 style config，并对 expand 取 `reverse()`（见 ExpandType.reverse）
2. 生成一对 “删除样式”的 StyleStart/End（其 info 使用 `get_style_flag_for_unmark`）

Rust 真值：`StyleConfigMap::get_style_flag_for_unmark` 与 `ExpandType::reverse`。

---

## 14.5 ExpandType 语义（边界插入继承）

### 14.5.1 ExpandType 枚举

- `before`：插入发生在 range.start 边界时，插入内容继承该样式
- `after`：插入发生在 range.end 边界时，插入内容继承该样式
- `both`：两侧都继承
- `none`：两侧都不继承

Rust 真值：`crates/loro-internal/src/container/richtext.rs::ExpandType`。

### 14.5.2 prefer_insert_before(anchor_type)

当插入位置恰好落在 style anchor 边界，需要决定“把新文本插到 anchor 的前还是后”，以实现 expand 语义。

规则（与 Rust 一致）：

- 对 Start anchor：
  - 若需要 expand_before，则新文本应插在 Start anchor **之后**
  - `prefer_insert_before(Start) = !expand_before`
- 对 End anchor：
  - 若需要 expand_after，则新文本应插在 End anchor **之前**
  - `prefer_insert_before(End) = expand_after`

Rust 真值：`TextStyleInfoFlag::prefer_insert_before`。

---

## 14.6 Unicode/UTF8/UTF16 换算要求

MoonBit 实现必须提供与 Rust 相同的边界换算行为：

- `unicode_to_utf8_index(s, unicode_index)`：若 index 越界返回 None
- `unicode_to_utf16_index(s, unicode_index)`
- `utf16_to_utf8_index(s, utf16_index)`
- `utf16_to_unicode_index(s, utf16_index)`：若不在边界，返回“前一个字符”的 unicode index（Rust 行为）

Rust 真值：`crates/loro-internal/src/container/richtext/richtext_state.rs` 中对应函数。

---

## 14.7 输出：toString / toDelta / toJSON

### 14.7.1 toString

- 返回纯文本内容（忽略样式 anchors）
- 必须与 Rust `RichtextState::to_string()` 行为一致

### 14.7.2 toDelta

输出形态对齐 Quill Delta（TS/WASM 行为）：

`DeltaSpan = { insert: String, attributes?: Map<String, Value> }`

规则：

- 相邻 text 片段若 attributes 相同应合并
- attributes 的值为样式 key->value（value 可为 bool/number/string/…）

Rust 真值：`crates/loro-internal/src/state/richtext_state.rs` 的 delta 相关逻辑（`DeltaRopeBuilder` + styles）。

### 14.7.3 toJSON / getShallowValue

与其它容器一致：

- shallow：样式 key/value 为普通值；若 value 内含容器引用，输出 `ContainerID` 字符串
- deep：递归展开容器引用（注意：RichText 内样式 value 也可能是容器引用）

---

## 14.8 实现落地建议（spec-driven）

建议分阶段：

1. 先只做纯文本（insert/delete/toString），忽略 mark（但保留 API 与错误提示）
2. 再实现 markStart/End 的持久化与正确配对
3. 最后实现 expand 行为与 toDelta

每阶段都用 Rust/TS 作为 oracle 做并发对照（见 17-test-plan）。

