# Heron UI · 三方向初稿（浅色 / 深色双主题）

> 依据：`ui/src` 现有 8 个视图、`stores/app.ts`、`api/index.ts`、`api/types.ts`，以及 Rust 侧真实能力
> （`resources/clients/*.toml` 五个客户端、`heron-source/src/market.rs` 五个市场、`doctor.rs` 四类检查、
> `upgrade.rs` 三方合并 + 冲突载荷）。三版用的是**同一套真实内容**（同一批 slug、同一组落点、同一组市场），
> 只换设计；所有文案都是 Heron 里真实存在的动作，没有编造数据。

## 一、三版怎么看

每个方向两个文件：默认打开是它的一方主题，右上/侧栏有切换按钮，**主题存在 localStorage（`heron.theme`），
可直接接到现有设置页的主题项**。

| 方向 | 文件 | 主题 | 截图 |
|---|---|---|---|
| ① 运维控制台 | `dir-1.html` / `dir-1-light.html` | 深色为默认，浅色为同一骨架的暖纸版 | `screens/dir-1-dark-*.png`、`screens/dir-1-light-*.png` |
| ② 编辑部 | `dir-2.html` / `dir-2-dark.html` | 浅色为默认，深色为同版夜景 | `screens/dir-2-light-*.png`、`screens/dir-2-dark-*.png` |
| ③ 信息密集台 | `dir-3.html` / `dir-3-dark.html` | 浅色为默认，深色为同版夜景 | `screens/dir-3-light-*.png`、`screens/dir-3-dark-*.png` |

八页都可点：导航、列表、复选框、开关、抽屉/弹窗、冲突合并、投影、收拢预览、Toast 都在。
路由用 `#overview` … `#settings`，可深链。

## 二、功能划分（三版共用）

现有 8 个平铺视图被收成三段心智：**收拢 → 分发 → 维护**。

| 段 | 页面 | 职责 | 现有视图 |
|---|---|---|---|
| 工作台 | 概览 | 健康快照 + 「接下来要你决定的事」+ 最近改动（可撤销） | OverviewView |
| 收集 · 分发 | 我的 Skill | 实体列表；每行直接给出「在哪些客户端生效 / 被压制 / 未扫描 / 坏链」 | LibraryView（verify 结果内联） |
| | 发现 | 未纳管清单 + dry-run 预览 + 保留原位置开关；病态嵌套只报告 | DiscoverView |
| | 来源与市场 | 五个市场（2 可搜索 / 3 仅浏览）+ 搜索结果 + 粘贴地址 | SourcesView |
| 维护 | 更新 | 6 种升级状态；冲突 → 逐文件合并（本地/上游并排，采用后保存重试） | UpgradesView + ConflictEditor |
| | 体检 | 只读检查，按「错误/警告/通过」分组，给建议操作 | DoctorView |
| | 项目与落点 | 客户端真正读取的目录（全局/项目）+ 开关 + 添加项目 | ProjectsView |
| | 设置 | 主题、语言、应用更新、客户端能力 | SettingsView |

**全局约定（三版一致）**
- 主题：浅 / 深两套，同一骨架只换纸墨；切换按钮常驻（①侧栏底 ②顶栏右侧 ③侧栏底）。
- 状态色只表达四种状态：生效中 / 被压制 / 未扫描 / 坏链；错误另用信号色。
- 危险操作（收拢、升级、合并）一律先 dry-run 预览，再说清备份位置。
- 语言：全部中文文案，可直接映射到现有 i18n key（新增键见文末）。

## 三、三张方向卡

### ① 运维控制台（深色优先）
- **逻辑与来源**：最佳设计师「深色专业工具」谱系——Linear 的信息密度与 Hairline 层级、Tidbyt 式安静底；浅色一侧取 Notion 暖纸的克制。
- **借的是什么**：212px 侧栏分三组；顶部只有面包屑 + 两个动作；指标不是四张卡，而是「接下来」清单 + 流水线读数；状态用 4 列圆点矩阵；底部常驻 git / 待处理健康条。
- **主角素材**：状态读数与路径本身（等宽字），无插画。
- **骨架**：左栏（分组导航 + 搜索 + 健康条）→ 顶栏（面包屑 + 主操作）→ 内容区（清单 + 读数 + 表格）。
- **胆量花在哪**：侧栏底的「健康条」——把 git、待处理、主题切换压成一排状态灯，一眼知道系统状态。
- **form 来自内容的哪里**：Heron 的本质是「一份实体 → 多个落点」，所以列表里每个客户端一列，而不是一句「已启用 3 处」。

### ② 编辑部（浅色优先）
- **逻辑与来源**：现实参照 Bloomberg Businessweek 媒体级粗野主义（web 样张 01，`assets/style-specimens/web/01-editorial-brutalism.jpg`）；字体关系取财经报道：标题巨号、正文小、右侧记分板列数字。
- **借的是什么**：纯黑/纯白 + 超链接蓝 + 信号红；3px 章头线切分栏目；模块之间 1px 规则线，不用卡片；标题 52px 压住 14px 正文；数字用 800 字重紧字距。
- **主角素材**：标题句本身（「你的 Skill 现在长什么样」「两边改的都留下」）。
- **骨架**：章头（刊名 + 栏目导航 + 操作）→ 标题带（左巨标题 + 右四项记分板）→ 模块（每个模块一个判断句标题）。
- **胆量花在哪**：巨号标题 + 右侧记分板占满首屏，信息不是平均铺开而是分主次。
- **form 来自内容的哪里**：Skill 管理是「状态日报」——每个页面顶上都该有一句话结论，而不是一个页面名。
- **说明**：这版替换了初稿的「田中几何册」（与 ③ 撞在零圆角细线家族，且侧栏竖排可读性差）。

### ③ 信息密集台（浅色优先）
- **逻辑与来源**：秒数轮盘 web #19「Swiss 极致黑白（Vercel 式）」+ east #09「田中一光几何平面构成」的一枚朱红；信息密度取 Are.na / Müller-Brockmann 栅格。
- **借的是什么**：纯黑白灰阶 + 一枚朱红只用于「需要决定」；12 栏模块网格，单元之间点线扫描行；数字 tabular；rail 用「序号 + 图标」。
- **主角素材**：客户端 × 落点状态矩阵（四格方块：实心/朱红/虚线）。
- **骨架**：104px 侧 rail → 章头（编号 + 标题 + 导语）→ 扫描行（扫描时间/git/锁文件/数据目录/待处理）→ 12 栏模块。
- **胆量花在哪**：扫描行与网格——把「这次扫描到底覆盖了什么」摆在每一页第一行。
- **form 来自内容的哪里**：Heron 的体检与投影本质是「矩阵问题」（Skill × 客户端），所以用方块矩阵而不是文字标签。
- **说明**：初稿侧 rail 只有英文缩写（8px），不满足可读性，已改成中文标签。

## 四、质量检查

- `design_lint.py check`（用 headless Chromium 实测字号/对比度/模板装饰）：三版红灯已清零；剩余黄灯说明：
  - ①②均有「A · B · C」中点连接——这里是 Heron 自己语义里的分隔（「收拢 · 分发 · 升级」），保留。
  - ①的等宽小字标签：路径与状态读数本来就该等宽，保留。
  - ③的 01–08 编号是模块索引（页面顺序即编号），保留。
- `compare`：dir-1 ↔ dir-2 ↔ dir-3 两两色板距离 0.43 / 0.735 / 0.037——②与③都是浅色编辑/瑞士谱系，靠标题字号与模块语言区分；如果两者都留下了，落地时应把②的底色改成真白、③保持 #F7F7F5 并再拉开（已在文件里这样处理）。
- 浅/深两两之间「骨架相关 1.0」是预期的：同一方向的两种主题共享骨架。
- 外观账本（`design_lint.py ledger add`）因沙箱不能写 `~/.huashu-design`，未能记账；不影响交付。

## 五、怎么接到现有代码

1. `ui/src/style.css`：把三版任一套的变量（`--bg/--surface/--border/--text/--muted/--accent/--ok/--warn/--error`）作为 `:root[data-theme=light|dark]`，删掉 `@media (prefers-color-scheme: dark)`，避免「跟随系统」和手动切换打架。
2. `stores/app.ts` 加 `theme: 'light' | 'dark'` + `toggleTheme()`，读写 `localStorage['heron.theme']`；`App.vue` 根节点设 `data-theme`。
3. `router.ts` / `App.vue` 按第二节的分组改写导航；`nav.*` 增加 `library` 分组键。
4. LibraryView 把 `api.verify()` 的结果按 slug × target 聚合到行内（现在只在「我的 Skill」里显示，且没有按客户端分列）。
5. 设置页加「主题」一项（i18n 已有 `settings.theme` 键，未使用）；`settings.language` 旁加密度选项（可选）。

## 六、新增 i18n 键（建议）

```json
{
  "nav": { "groupWorkbench": "工作台", "groupPipeline": "收集 · 分发", "groupMaintain": "维护" },
  "overview": { "next": "接下来", "pipeline": "流水线", "recent": "最近改动", "undo": "撤销", "health": "健康" },
  "library": { "visMatrix": "落点状态", "drawer": "详情", "reproject": "重新投影" },
  "discover": { "dryRunTitle": "收拢预览", "ignoreNested": "忽略嵌套" },
  "upgrades": { "mergeFile": "逐文件合并", "baseline": "基线", "compare": "比较" },
  "settings": { "themeLight": "浅色", "themeDark": "深色", "density": "界面密度" }
}
```
