# Undo 性能优化方案

## 问题背景

当前的 undo 实现存在性能问题，主要体现在以下几个方面：

### 当前实现的问题

1. **存储结构简单**：undo stack 中只存储 idspan，缺乏操作的详细信息
2. **执行开销高**：每次 undo 操作需要执行复杂的版本切换流程
3. **时间复杂度问题**：连续 N 次 undo 操作的时间复杂度为 O(n²)

### 执行流程分析

当前的 undo 执行流程如下：

```
当前版本(b) → 目标版本(a) → 前一版本(ap) → 计算diff → 回到版本(b) → 应用diff
```

具体步骤：

1. 从当前版本 `b` checkout 到版本 `a`
2. 从版本 `a` checkout 到前一版本 `ap` 以计算 diff
3. 从版本 `ap` 回到版本 `b`
4. 在版本 `b` 上应用计算出的 diff batch

这种方式导致：

- 每个 undo 步骤都需要多次 checkout 操作
- 随着 undo stack 深度增加，性能急剧下降
- redo 操作同样存在性能问题

## 优化方案

### 核心思路

在本地操作 apply 时立即计算出能撤销该操作的 diff batch，避免后续的复杂计算。

### 不同容器类型的处理

#### Map 容器

- **插入操作** → **撤销操作**：删除刚插入的 key-value
- **更新操作** → **撤销操作**：恢复到之前的 value
- **删除操作** → **撤销操作**：重新插入被删除的 key-value

#### Tree 容器

- **移动节点** → **撤销操作**：将节点移动回原位置
- **插入节点** → **撤销操作**：删除刚插入的节点
- **删除节点** → **撤销操作**：重新插入被删除的节点

#### Text 容器

- **删除文本** → **撤销操作**：插入被删除的文本内容
- **插入文本** → **撤销操作**：删除刚插入的文本
- **格式标记**：需要考虑富文本标记的恢复（评论等标记可能不需要恢复）

#### List 容器

- **移动元素** → **撤销操作**：将元素移动回原位置
- **其他操作**：类似于其他容器的处理方式

### Stack 管理

#### Undo/Redo Stack 的对偶性

```
执行操作 → 生成 undo diff batch → 推入 undo stack
执行 undo → 生成 redo diff batch → 推入 redo stack
执行 redo → 生成 undo diff batch → 推入 undo stack
```

这种设计保证了 undo 和 redo 操作的一致性和对称性。

## 实现方案

### 第一步：新增 Subscriber 类型

**目标**：创建专门用于订阅 undo diff batch 的 subscriber

**实现要点**：

- 该 subscriber 仅用于内部，不对外暴露
- 与 undo manager 配合工作
- 各个容器类型在 apply 本地操作时需要支持该 subscriber

**具体要求**：

- 当有 undo subscriber 订阅时，容器在执行本地操作时发出 undo event
- 外部能够接收并汇集这些 event，发送给 subscriber
- 实现测试：验证每次文档 commit 时能收到对应的 undo diff batch，并能通过执行该 batch 撤销 commit

### 第二步：修改 Undo Manager

**目标**：将 undo stack 和 redo stack 的存储类型从 idspan 改为 diff batch

**实现要点**：

- 利用第一步实现的 undo diff batch 订阅机制
- 更新 stack 的数据结构
- 修改 undo/redo 的执行逻辑

### 第三步：行为一致性测试

**目标**：确保新实现与旧版本的 undo/redo 行为一致

**测试方案**：

- 对比新旧版本的 undo/redo 行为
- 编写手动测试用例
- 考虑添加 fuzzing test（可选）

## 预期收益

1. **性能提升**：消除多次 checkout 操作，显著降低时间复杂度
2. **响应速度**：undo/redo 操作更加快速响应
3. **扩展性**：为复杂文档操作提供更好的性能基础
4. **用户体验**：在大型文档中进行频繁 undo/redo 操作时体验更流畅
