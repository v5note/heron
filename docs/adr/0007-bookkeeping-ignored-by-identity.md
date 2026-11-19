# ADR-0007: Heron 自身的记账文件不参与内容指纹

- 状态：已采纳（2026-11-19）
- 相关：设计 §10.3、实现文档 §5.4

## 背景

收拢后 store 里的副本会多出 `.gitignore` 与 `heron.skill.json`。指纹算法最初只忽略
`heron.skill.json`，于是"收拢前"与"收拢后"的内容哈希不同，导致：

* 同内容去重（adopt 的 duplicate 检测）永远失败；
* 升级时的"本地改动"判定可能被自身记账文件污染。

## 决策

`HASH_IGNORED_NAMES` 增加 `.gitignore`（其余：`.git`、`.heron`、`desktop.ini`、`.DS_Store`、`Thumbs.db`、`heron.skill.json`）。
即：**Heron 自己写入的文件永远不影响 skill 的身份**。

## 后果

- 收拢前后哈希一致，去重与本地改动检测按预期工作；
- 用户在 skill 里自写的 `.gitignore` 不会被跟踪进哈希——
  这是有意的取舍：它属于构建噪声而非 skill 内容。
