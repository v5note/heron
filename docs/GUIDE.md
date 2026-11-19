# 上手指南

> [Back to README](../README.md) · [中文说明](../README.zh-CN.md)

这份指南假设你的机器上 Skill 已经散落成形（大多数人是这样），目标是把它收成一套可管理的资产。
全文的命令都可以直接复制运行。

---

## 0. 先看看现状

```bash
heron scan
```

```text
scanned 219 directories in 79 ms
  skills          52
  legacy commands 0
  nested suspects 0   (reported only - nothing is moved)
  needs attention 0
  skipped (not found) ~/.heron/skills
  skipped (not found) ~/.cursor/skills
```

四个数字分别是：识别到的 Skill 数、旧式斜杠命令数、疑似病态嵌套数、frontmatter 有问题的数量。
"skipped (not found)" 是正常的——那些客户端你没装。

接着看清单：

```bash
heron detected
```

```text
pdf                          agents    global   skill            ~/.agents/skills/pdf/SKILL.md
review                       claude    global   legacy-command   ~/.claude/commands/review.md
pdf                          claude    global   skill            ~/.claude/skills/pdf/SKILL.md

   1  legacy-command
   2  skill
```

注意这里出现了**两条 `pdf`**：它们在不同目录里，内容相同。这正是 Heron 要去重的情况。

---

## 1. 先预览，再收拢

**永远先 `--dry-run`。** 它会告诉你每一步要做什么，但不碰磁盘：

```bash
heron adopt --all --dry-run
```

```text
would adopt pdf
  - {"action":"create_store","slug":"pdf","from":"…/.agents/skills/pdf"}
  - {"action":"backup","from":"…/.agents/skills/pdf","to":"<backups>/adopt"}
  - {"action":"link_original_path","path":"…/.agents/skills/pdf/SKILL.md","target":"…/store/pdf"}
would adopt review
  - {"action":"create_store","slug":"review","from":"…/.claude/commands/review.md"}
  - …
would adopt pdf (same content as pdf: no second copy)
```

最后一行说明：**第二个 `pdf` 不会再复制一份**，直接把原位置指向已有实体。

确认没问题后执行：

```bash
heron adopt --all
```

事后验证原目录变成了链接：

```bash
ls -la ~/.agents/skills/
# pdf -> /Users/you/Library/Application Support/io.greyheron.heron/store/pdf
```

### 只想收拢指定的几个

```bash
heron adopt pdf review
```

### 不想动原位置

如果你还在用别的方式管理某个目录，加 `--keep-original`：只登记进 Heron，原目录原样保留。

```bash
heron adopt pdf --keep-original
```

> 区别：默认模式下原位置会变成指向存储区的链接（改一处处处生效）；
> `--keep-original` 则是两份独立内容，之后可能漂移——`heron doctor` 会提醒你。

---

## 2. 决定"让哪个客户端看到"

收拢只代表 Heron 接管了，**不代表客户端加载它**。先确认：

```bash
heron verify
````

```text
pdf      heron:global     symlink   visible
review   claude:global    symlink   visible
```

- `visible` — 客户端会加载
- `not scanned` — 文件在，但不在该客户端的扫描路径里（等于白装）
- `shadowed` — 同名 Skill 被优先级更高的（例如插件）压过
- `broken` — 链接断了

可用的落点：

```bash
heron targets
```

```text
* heron:global           heron      ~/.heron/skills
  agents:global          agents     ~/.agents/skills
  claude:global          claude     ~/.claude/skills
  codex:global           codex      ~/.codex/skills
  cursor:global          cursor     ~/.cursor/skills
  opencode:global        opencode   ~/.config/opencode/skills
```

`*` 是默认落点（`~/.heron/skills`，由 Heron 自己拥有，不受任何客户端升级影响）。

想装到某个客户端自己的目录：

```bash
heron project pdf --on claude:global
heron project pdf --on agents:global
```

### 投影方式

| 方式 | 说明 | 适用 |
|---|---|---|
| `symlink`（macOS 默认） | 零同步 | 大多数客户端 |
| `junction`（Windows 默认） | 零权限、零同步 | Windows 普通用户 |
| `copy` | 真实复制 | 不认链接的扫描器 |
| `pointer` | 生成一个指向真实目录的 `SKILL.md` | 只读 `.md` 的扫描器 |

```bash
heron project pdf --on claude:global --mode copy
```

> 如果某个客户端明明装了却显示 `not scanned`，多半是它不认链接——换成 `copy` 即可。

---

## 3. 从 GitHub 或市场安装

### 直接给地址

```bash
# 仓库里的某个子目录
heron install https://github.com/anthropics/skills/tree/main/skills/pdf

# 简写：owner/repo#ref:path
heron install github:anthropics/skills#main:skills/pdf
```

先看它打算装什么：

```bash
heron install https://github.com/anthropics/skills --subpath skills/pdf --dry-run
```

如果仓库里有多个 Skill 而你没指定 `--subpath`，Heron 会**明确报错并列出候选**，不会替你猜：

```text
error[E_SOURCE_AMBIGUOUS]: {"count":"2","examples":"skills/pdf, skills/tdd","reason":"multiple_skills"}
```

### 通过市场

```bash
heron market list
heron market search skills_sh pdf
```

```text
+ anthropics/skills/pdf                    skills · pdf
  https://www.skills.sh/anthropics/skills/pdf
+ openai/skills/pdf                        skills · pdf
```

`+` 表示 Heron 能直接安装，`~` 表示只能打开浏览器（把 Git 地址粘回来用 `heron install`）。

---

## 4. 升级，且不丢自己的改动

### 先看上游有没有动

```bash
heron outdated
```

```text
pdf      diverged (local + upstream)   1.0.0
tdd      up to date                    1.0.0
review   not tracked                   0.0.0
```

- `diverged` — 你改过，上游也改了，可以合并
- `not tracked` — 本地收拢来的，没有上游（`0.0.0` 是占位，不是真的版本号）

### 看看自己改了什么

```bash
heron diff pdf
```

```text
✎ SKILL.md
  + MY-LOCAL-LINE
  - line-c
```

### 升级

```bash
heron upgrade --all
```

这是**三方合并**：你的改动的和上游改动都会保留。合并结果里能看到两边的内容：

```text
upgraded pdf 153e9707 -> 30edb861
```

### 真的冲突了

如果两边改了同一行：

```bash
heron upgrade pdf
```

```text
conflict in pdf:
  SKILL.md
  the skill was left untouched; resolve it manually or roll back
```

**注意"left untouched"**：冲突时 Heron 不会把你的文件写成带 `<<<<<<<` 标记的样子。

查看两边内容并选择：

```bash
heron conflict pdf      # 列出冲突文件
heron resolve pdf --take theirs   # 或 --take ours
```

桌面应用里可以**逐个文件**选择本地或上游，再"保存并重试升级"。

### 回滚

```bash
heron rollback pdf --to 1.0.0
```

回滚本身也会被记录（打一个 `heron-rollback-backup-*` 标签），所以可以再滚回来。

---

## 5. 让团队保持一致

### 导出

```bash
heron lock export --dir ~/work/my-project
```

生成 `heron.lock`，记录每个 Skill 的来源、提交、版本与内容哈希——**不记录绝对路径**，
所以可以提交进仓库、跨机器使用。

### 在新机器上复现

```bash
heron lock diff --file heron.lock    # 先看差什么
heron lock apply --file heron.lock   # 缺的自动装
```

```text
installed 1  up to date 3  drift 0
```

之后如果本地有人改了某个 Skill，`lock diff` 会报 `ContentChanged`。

---

## 6. 定期体检

```bash
heron doctor
```

```text
warn    nested-suspect      ~/.agents/skills/huashu/.agents/skills/inner/SKILL.md
error   broken-link         ~/.claude/skills/old-skill
```

- **病态嵌套**只会被报告，Heron 不会去动你的目录结构。
- **坏链**可以 `heron project <slug> --on <target>` 重建，或 `heron unproject` 清掉。

---

## 7. 桌面应用

界面把上面的流程图形化了：

| 页面 | 做的事 |
|---|---|
| 概览 | 统计 + 一键扫描 / 体检 |
| 我的 Skill | 已纳管的 Skill 及其在各客户端的可见性 |
| 发现 | 未纳管清单，勾选后收拢 |
| 来源与市场 | 五个市场搜索，区分"可安装 / 仅浏览器" |
| 项目 | 自动发现含客户端配置的项目 |
| 更新 | 检查更新、升级、冲突时打开编辑器 |
| 体检 | 问题列表与建议 |
| 设置 | 语言、git 状态、客户端能力表 |

界面与命令行操作的是同一份数据，随便用哪个。

---

## 常见疑问

**Q: 我的文件被移到哪了？**
收拢后实体在数据目录的 `store/<slug>/`，原位置是指过去的链接。备份在 `backups/`。

**Q: 能撤销收拢吗？**
备份是操作前的完整副本，手工拷回即可。Heron 目前不提供一键撤销收拢（设计里是可选项）。

**Q: `~/.agents/skills` 会被 Heron 接管吗？**
不会。Heron 的默认落点是 `~/.heron/skills`；`.agents/skills` 只作为可选落点，
并且 Heron 只碰它自己创建的条目，不动 `npx skills` 装的东西。

**Q: 为什么有些 Skill 显示 `not tracked`？**
它们是从本机收拢来的，没有上游仓库。只有通过 `heron install` 安装的 Skill 才有升级信息。

**Q: 卸载会不会删掉我的 Skill？**
不会。卸载不删除数据目录；要彻底清理需要在界面里显式确认。
