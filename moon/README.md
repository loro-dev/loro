# Moonbit Loro Codec

本目录包含用 Moonbit 实现的 Loro 二进制编码格式编解码器（对应 `docs/encoding.md`）。

## 目录结构

- `moon/loro_codec/`：核心库（编解码、校验、压缩、SSTable、ChangeBlock 等）
- `moon/cmd/loro_codec_cli/`：命令行工具（用于 e2e 转码与调试）
- `moon/specs/`：实现计划与数据结构设计文档

## 开发约定

- 先实现基础模块（bytes/leb128/postcard/xxhash32/lz4），再实现 SSTable 与 ChangeBlock。
- 以 Rust ↔ Moon 的 e2e 互通为最终验收。
