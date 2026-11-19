# ADR-0004: 显式数据目录拥有全部可写目录

- 状态：已采纳（2026-11-19）
- 相关：实现文档 §4.1

## 背景

`--data-dir` 最初只重定向 `data/` 与 `log/`，`config/` 与 `cache/` 仍指向系统默认位置。
在沙箱/CI 中这导致 `Operation not permitted`：程序试图写 `~/Library/Application Support`。

## 决策

一旦用户（或测试、CI）显式提供数据目录，**四个目录全部置于其下**：

```
<data>/           数据（store、db、backups、trash）
<data>/config/    配置
<data>/cache/     缓存
<data>/log/       日志
```

未提供时仍按平台规范解析（macOS Application Support / Windows LOCALAPPDATA / Linux XDG）。

## 后果

- 便携安装、临时环境与测试不再触碰系统目录；
- "配置"和"数据"合并到一个可整体备份/搬迁的根；
- 与 design ADR-8 不冲突：默认路径仍遵循平台规范。
