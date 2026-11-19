# ADR-0002: 平台层的路径指纹改用 FNV-1a，blake3 只用于内容指纹

- 状态：已采纳（2026-11-19）
- 相关：实现文档 §4.2 / §5.4

## 背景

`heron-platform` 用 `blake3::hash()` 生成 `project_key`（项目 target id 的短哈希）。
交叉编译 Windows 目标时，`blake3` 的 build script 需要一个支持 `/arch:AVX512` 的 C 编译器，
导致 `cargo check --target x86_64-pc-windows-msvc` 在 macOS 上失败——而这正是我们用来
在本地验证 Windows 代码路径的手段。

## 决策

- `project_key` 改为 FNV-1a（纯 Rust，无 C 依赖），输出 8 位十六进制；
- `blake3` 仍作为**内容指纹**的首选（在 `heron-scan` 中使用，设计文档 §10.3 不变），
  但不再进入平台层。

## 后果

- 平台层可以在任何主机上交叉编译检查，Windows 代码路径可在 macOS/Linux 上静态验证；
- `project_key` 的碰撞只影响 target id 唯一性，且由数据库 UNIQUE 约束兜底报错；
- 若未来需要抗碰撞的 project id，可换回 blake3 并接受 C 工具链依赖。
