# ADR-0006: 投影身份按"位置"而非 slug；客户端目录自动成为目标

- 状态：已采纳（2026-11-19）
- 相关：实现文档 §7.1、设计 §10.15

## 背景

真机演练中发现两个问题：

1. 同一个 skill 同时存在于 `~/.agents/skills/wechat-mp` 与 `~/.claude/skills/wechat-mp` 时，收拢后两处都变成了
   指向 store 的链接，但索引里两条投影记录都用 `link_name = "wechat-mp"`，
   撞上 `UNIQUE(target_id, link_name)` 约束。
2. `verify` 把这些"原位置链接"按目标目录（默认落点）去判定，误报 `not scanned`；
   而它们本来就在客户端的扫描目录里，实际是可见的。

## 决策

1. 投影的 `link_name` 表示**位置**而非 slug：收拢产生的投影记为 `<client>@<path-hash-8>`；
   用户显式投影到目标目录时仍用 slug。
2. `verify` 从 `link_name` 的前缀识别客户端，用**该客户端**的扫描路径判定可见性；
   没有前缀的记录按目标目录判定。
3. 首次引导把每个客户端的全局目录都登记为 target（`claude:global` 等，`discover=auto`），
   默认落点 `heron:global` 仍是唯一 `is_default`。

## 后果

- 同一 skill 的多份拷贝可以同时存在且都被正确判定为可见；
- 用户可以直接 `heron project <slug> --on claude:global`，无需先手工建目标；
- 项目级目标仍在 M2 随项目注册产生。

## 修订（2026-11-23）

`~/.heron/skills` 改为存储区本身（唯一真相源），不再是投影目标：决策 3 里的 `heron:global`
已随之删除，系统不再存在自动创建的默认目标（`is_default` 只能由用户自己设定）。
决策 1 与 2 不受影响；收拢时若链接所在目录尚未登记，按 `<client-kind>:<path-hash-8>` 就地登记。
