# ADR-0010: GUI 是薄壳，所有逻辑留在 service 层

- 状态：已采纳（2026-11-19）
- 相关：设计 ADR-1/§12、实现文档 §7.5/§8

## 背景

M4 要交付桌面应用。最容易走偏的做法是把业务逻辑塞进 Tauri command，
导致 CLI 与 GUI 行为分叉——同一个操作在两边结果不同，是最难排查的缺陷。

## 决策

1. `src-tauri` 只做三件事：解析参数、加锁、调用 `heron-service`、序列化结果；
   它不包含任何文件系统或 git 操作。
2. 每个 command 与 CLI 的某个子命令一一对应（如 `project_skill` ↔ `heron project`），
   因此 CLI 的每一个用例都被 GUI 复用。
3. 前端不直接访问文件系统：`capabilities/main.json` 只开 `core:default` 与 `opener:default`，
   **没有 `fs:` 权限**；所有路径由 Rust 侧处理。
4. 错误以 `Msg{code, params}` 跨边界，前端用 `i18n` 渲染（CI 校验中英 key 一致，当前 114 个）。

## 后果

- 任何行为修复只需改一处，CLI 与 GUI 同时受益；
- 前端可以脱离 Rust 单独开发（`pnpm dev`），也可以被未来替换；
- 安全边界清晰：WebView 拿不到文件系统权限。
