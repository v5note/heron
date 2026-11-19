# 命令参考

所有命令都支持：

| 参数 | 说明 |
|---|---|
| `--json` | 输出结构化 JSON（脚本用） |
| `--data-dir <路径>` | 使用指定的数据目录（隔离测试、便携安装） |

退出码：`0` 成功 / `1` 一般错误 / `2` 用法错误 / `3` 需要用户决策（例如升级冲突）。

---

## 发现与索引

### `heron scan [--roots <路径>...] [--max-dirs <n>] [--verbose]`

扫描所有已知位置并刷新索引。

- `--roots` 追加要扫描的项目根（会当作项目登记）
- `--max-dirs` 限制遍历目录数（默认 50000，触发时输出 `!` 提示）
- `--verbose` 列出全部命中项

### `heron detected [--state <状态>] [--all]`

列出未纳管条目。`--state` 可取 `candidate`（默认）/ `ignored` / `adopted` / `conflict` / `all`。
`--all` 包括病态嵌套项。

---

## 收拢

### `heron adopt <slug|path>... | --all [--dry-run] [--keep-original]`

| 参数 | 说明 |
|---|---|
| `--all` | 收拢所有候选（病态嵌套项除外） |
| `--dry-run` | 只打印计划，不落盘 |
| `--keep-original` | 保留原位置，只登记 |

行为约定：

- 先备份到 `<数据目录>/backups/<时间>/adopt-<slug>/`，失败即中止
- 内容相同的多份拷贝只保留一份，其余位置改为指向它
- `.claude/commands/*.md` 只取**该文件**，不会把整个 `commands/` 目录拷进来
- 默认结束时原位置变成链接；`--keep-original` 时原位置不动

---

## 分发

### `heron project <slug> [--on <target>] [--mode copy|symlink|junction|pointer] [--dry-run]`

投影到某个落点。不指定 `--on` 时用默认落点（`heron:global`）。

可能的结果：

| 情况 | 行为 |
|---|---|
| 目标位置为空 | 创建投影 |
| 已是本 Skill 的链接 | 无操作（幂等） |
| 是**别人的实体目录** | `E_NOT_MANAGED`，不覆盖 |
| 是别人的链接 | `E_DRIFTED`，不覆盖 |
| copy 模式且内容被改过 | `E_DRIFTED`，不覆盖 |

### `heron unproject <slug> [--on <target>]`

移除投影，**保留存储副本**。copy 模式下若目标内容被改动过，会拒绝删除。

### `heron targets`

列出所有落点，`*` 标记默认落点。

### `heron verify [<slug>]`

— 逐个投影判定可见性

| 状态 | 含义 | 怎么办 |
|---|---|---|
| `visible` | 客户端会加载 | — |
| `not scanned` | 不在扫描路径内 | 换目标或改投影方式 |
| `shadowed` | 同名被更高优先级压过 | 改名或禁用对方 |
| `restart required` | 需重开客户端会话 | 重启 |
| `broken` | 链接失效 | 重新 project |

---

## 安装与升级

### `heron install <输入> [--ref <分支/标签>] [--subpath <路径>] [--as <slug>] [--dry-run]`

支持的输入形式：

| 形式 | 示例 |
|---|---|
| GitHub 简写 | `anthropics/skills` |
| tree 链接 | `https://github.com/o/r/tree/main/skills/pdf` |
| blob 链接 | `https://github.com/o/r/blob/main/skills/pdf/SKILL.md` |
| 带前缀 | `github:o/r#main:skills/pdf` |
| SSH | `git@github.com:o/r.git` |
| 本地 git 仓库 | `/path/to/repo.git` 或 `file:///path/to/repo.git` |

GitLab 的多级子组会产生歧义，Heron 会列出候选而不是猜。

### `heron outdated`

对比每个 Skill 的上游提交（会先刷新缓存）。状态：

| 状态 | 含义 |
|---|---|
| `up_to_date` | 上游没变，本地也没改 |
| `has_update` | 只有上游变了 |
| `local_modified` | 只有本地改了 |
| `diverged` | 两边都改了（可合并） |
| `pinned` | 被固定版本锁住 |
| `upstream_gone` | 上游分支/仓库不见了 |
| `not_tracked` | 本地收拢来的，无上游 |

### `heron upgrade [<slug>...] [--all] [--dry-run]`

三方合并。**冲突时工作区保持原样**，退出码 3。

### `heron rollback <slug> --to <版本|标签>`

回到某个已落地的版本。回滚前自动打 `heron-rollback-backup-<sha>` 标签。

### `heron diff <slug> [--against <标签>]`

显示相对基线的改动（**包含未提交的修改**）。`--against` 可与某个历史版本比较。

### `heron conflict <slug>`

列出升级会发生冲突的文件，以及两边内容（`--json` 时含完整文本，供编辑器使用）。

### `heron resolve <slug> [--take ours|theirs]`

对每个冲突文件采用一侧内容并**重试升级**。桌面应用支持逐文件选择。

---

## 市场

### `heron market list`

五个内置市场及其能力：

| 市场 | 能力 |
|---|---|
| `skills_sh` | 可搜索 + 可安装（解析出对应 GitHub 仓库） |
| `clawhub` | 可搜索；条目多为平台托管，安装时转为引导 |
| `qoder` / `skillhub_cn` | 仅浏览器 |
| `skillsmp` | 仅浏览器（默认关闭） |

### `heron market search <市场> <关键词> [--limit <n>]`

`+` 可安装，`~` 只能打开浏览器。

### `heron market plan <市场> <key>`

打印该条目的安装方式（生成一条 `heron install` 命令，或给出要打开的网址）。

---

## 团队协作

### `heron lock export [--dir <目录>] [--print]`

写出 `heron.lock`。`--print` 直接打印到标准输出。

### `heron lock diff --file <路径>`

对比锁文件与本地存储：`Missing` / `ContentChanged` / `SourceChanged`。

### `heron lock apply --file <路径> [--dry-run]`

安装锁文件里缺失的 Skill。

---

## 项目与维护

### `heron projects [--roots <路径>...] [--register] [--depth <n>]`

发现含 `.<client>/skills` 的项目根。`--register` 登记为项目落点
（**投影默认关闭**，需要显式启用）。

### `heron doctor`

只读体检，发现问题时退出码为 1。

---

## 错误码

每个错误都带稳定的 code 与参数，界面按语言渲染。

| 错误码 | 含义 | 处理 |
|---|---|---|
| `E_GIT_MISSING` | 找不到 git | 安装 git（macOS: `xcode-select --install`；Windows: `winget install Git.Git`） |
| `E_NETWORK` | 网络/认证失败 | 检查代理、SSH key；可重试 |
| `E_SOURCE_AMBIGUOUS` | 地址有歧义 | 用 `--subpath` 或带 `#ref:path` 的显式形式 |
| `E_SLUG_CONFLICT` | 同名 Skill 已存在 | 换 `--as`，或改用 `upgrade` |
| `E_REF_NOT_FOUND` | 分支/标签/路径不存在 | 核对仓库结构 |
| `E_DRIFTED` | 目标被外部改动 | 先备份再决定是否覆盖 |
| `E_NOT_MANAGED` | 该位置不归 Heron 管 | 先 `adopt` 再 `project` |
| `E_SAFETY_REFUSED` | 为安全拒绝 | 检查是否在动实体目录 |
| `E_FRONTMATTER` | SKILL.md 不合规 | 修 `name`/`description` |
| `E_MARKET_UNAVAILABLE` | 市场不可达 | 稍后重试（适配器会重试 3 次） |
| `E_MERGE_CONFLICT` | 合并冲突 | `heron conflict` + `resolve` |
| `E_PINNED` | 版本被锁 | 先解除 pin |
| `E_NATIVE_SCAN_DISABLED` | 原生扫描开关未开 | 在设置里显式开启 |
| `E_NATIVE_SCAN_UNSUPPORTED` | 该客户端未经证实支持 | 不会写入，属预期 |
| `E_DB_CORRUPT` | 索引损坏 | 删除 `heron.db` 后重新 `scan`（可从存储区重建） |

---

## 环境隔离

目前只有 `--data-dir` 一个手段（没有环境变量开关）：

```bash
heron --data-dir /tmp/heron-test scan    # 完全隔离的一套数据
```

它同时接管 config / cache / log，因此不会碰系统目录——测试与便携安装都用这个方式。

---

## 配置与数据布局

```text
<数据目录>/
├── heron.db          索引（可从 store 重建）
├── store/<slug>/     每个 Skill 一个 Git 仓库（唯一真相源）
├── backups/          收拢/覆盖前的完整副本
├── sources-cache/    远端裸仓缓存（可安全删除）
├── staging/          安装过程的临时区
├── trash/            软删除区
└── log/
```

客户端路径与能力表位于仓库的 `resources/clients/*.toml`，每条都带 `verified_by` 标记来源。
