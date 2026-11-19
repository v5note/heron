# ADR-0003: 链接层的安全契约

- 状态：已采纳（2026-11-19）
- 相关：设计 §10.2 / 风险 R3、实现文档 §4.3 / §10.2

## 决策

`heron-platform::link` 是**唯一**可以创建/删除投影落点的模块，并遵守：

1. `remove_link` 只接受链接；实体目录/文件返回 `E_SAFETY_REFUSED`，绝不删除；
2. Windows 上 junction 的解除走 `RemoveDirectoryW`（只摘 reparse point），
   失败时回退到 `FSCTL_DELETE_REPARSE_POINT` + `remove_dir`（非递归）；
   **任何路径都不得使用 `remove_dir_all`**；
3. 创建前要求目标存在且为目录，否则 `E_LINK_TARGET_MISSING`；
4. 重复链接到同一目标是幂等的（`LinkOutcome::AlreadyPresent`）；
5. 请求 symlink 但无权限（`ERROR_PRIVILEGE_NOT_HELD`）时自动降级为 junction，并返回 `E_LINK_PRIVILEGE` 警告。

## 验证

- `tests/link_safety.rs`（全平台，10 例）与 `tests/link_safety_windows.rs`（Windows，L1–L6）
  每条用例都在目标里写"哨兵文件"，操作后断言其存在；
- Windows 用例在 CI 中必须以**普通用户**身份运行。
