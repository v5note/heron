# ADR-0008: 升级时导入上游对象；版本标签打在本地位提交上

- 状态：已采纳（2026-11-19）
- 相关：实现文档 §5.5/§5.6、设计 §10.4

## 背景

实现 `upgrade` 时遇到三个真实的 Git 语义问题（均由端到端测试暴露）：

1. **上游提交不在技能仓库里**。每个技能有自己的 `.git`，而 `theirs` 是远端缓存中的 sha，
   直接 `git tag v1.0.0 <remote-sha>` 会报 *nonexistent object*。
2. **浅取过来的提交没有父链**。`--depth=1` 的缓存里，被导入的上游提交可能读不到它的 tree，
   `merge-tree` 报 *unable to read tree*。
3. **`--merge-base` 接受的是 tree 而不是 commit**，传 `<commit>^` 会报 *could not parse as tree*。

## 决策

1. 升级前用 `SkillRepo::import_objects(cache_path, sha)`（即 `git fetch <cache> <sha>`）
   把**新上游提交**与**基线提交**都导入技能仓库；
2. `merge_from_upstream(theirs, base)` 内部把 `base` 解析为 `<base>^{tree}` 再传给
   `merge-tree --merge-base`；
3. **版本标签打在本地提交上**：`v<version>` 指向"Heron 落地该上游版本的那个本地提交"，
   上游 sha 记录在 `heron.skill.json` 的 `baseline.commit`；回滚用本地标签，语义明确；
4. 版本号解析优先级：**上游 tag → `SKILL.md` 的 `metadata.version` → `0.0.0+<sha>`**。

## 验证

`crates/heron-service/tests/remote_flow.rs` 覆盖：安装 → 上游更新 → 本地修改后升级（两边改动合并）
→ 同一行冲突（工作区保持原样）→ 回滚到旧标签 → 版本号保持语义化。
