# ADR-0011: 差异对比以本地基线为准；冲突解决先提交再重试

- 状态：已采纳（2026-11-19）
- 相关：设计 §10.4、实现文档 §5.5 / §13

## 背景

实现 `heron diff` 与冲突解决时，暴露出四个相互关联的问题（都由端到端测试抓到）：

1. **上游提交不在技能仓库里**：`before` 侧读不到，`git show <upstream-sha>:SKILL.md` 报
   *path exists on disk, but not in <sha>*。
2. **未提交的改动不算改动**：`changed_since_baseline` 只比较 `baseline..HEAD`，
   用户直接改文件（不 commit）时 diff 为空。
3. **浅取的历史不足以三方合并**：`--depth=1` 抓下来的上游提交没有父链，
   `merge-tree` 判断为 *refusing to merge unrelated histories*，退化成"干净"的假合并。
4. **缓存是陈旧的**：`conflict_payload` 直接读缓存 ref，上游刚发布的新版本看不到。

## 决策

1. `heron.skill.json` 的 `baseline` 增加 **`local_commit`**：安装时记录"本地落地该上游版本的提交"，
   diff 与回滚都以它为准（它一定存在于本地仓库）；上游 sha 仍保留作溯源。
2. `changed_since_baseline` 合并 `git diff baseline..HEAD` 与 `git status --porcelain`，
   **未提交改动与新文件都算改动**。
3. 缓存抓取改为 `--depth=200 --tags`：既能三方合并（有真实祖先链），又不至于克隆整库。
4. 任何读取上游状态的操作（`outdated`、`conflict`、`upgrade`）都先 `ensure()` 刷新缓存。
5. 冲突解决流程：先 `commit_all` 把本地改动变成"ours"，再取上游快照作为"theirs"；
   用户选择后写回并提交，然后**重试 upgrade**——而不是手工构造合并。

## 后果

- `heron diff` 能显示用户真实的工作区改动；`heron conflict` 能列出真实冲突文件；
- `heron resolve --take theirs|ours` 可用，GUI 可用同一份 `conflict_payload` 构建三栏编辑器；
- 缓存体积比 `--depth=1` 略大（200 个提交的历史），换取正确的合并语义。
