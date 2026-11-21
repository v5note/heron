# Heron 扫描进度交互 + 概览页改版 · 设计 spec（三方向共同输入）

## 项目是什么
Heron（苍鹭）是一个 macOS 桌面应用（Tauri 2 + Vue 3），管理本机各 Agent 客户端的本地 Skill。
本次设计两件事，输出为可交互 HTML 原型（桌面界面原型，非生产代码）：

1. **扫描本机进度交互**：现状是点「扫描本机」后整个应用卡死（后端同步遍历文件系统）。
   已批准的方案（不重开讨论）：
   - 按钮原地变形为进度胶囊：`扫描本机` → `⟳ 扫描中 2/6`（root 级确定进度）→ hover/点击变 `点击取消 · 5/6` → 完成后短暂显示结果再复位。按钮宽度固定，不抖动。
   - 顶栏下方 2px 全局进度轨（indeterminate 流光），扫描期间跨页面存活。
   - 增量结果流入：发现项以 ~300ms 批次流入（发现页卡片流 + 侧栏计数 +3 跳动）。
   - 阶段文案：准备 → 遍历 → 解析 → 入库 → 汇总。root 内不伪造百分比，root 级是真实 2/6。
   - 4 个状态必须可在原型里真实点出来：空闲 / 扫描中 / 取消待命（hover 或再点）/ 完成+结果流入。
   - 完成态 toast 可操作（「52 个新发现」→ 跳发现页）；truncated 时明示「结果不完整」+ 链接到排除设置；空闲态显示「上次扫描 N 秒前 · 52 个 Skill」。
2. **概览页重新设计**：概览回答「现在需要我决定什么」。现有内容：接下来（待办列表）、流水线（已纳管/启用落点/可升级 + 投影比例条）、落点状态表。改版要解决：信息层级平、扫描状态无处安放、数字不呼吸。

## 受众与场景
开发者本人日常运维本机 Skill：打开 → 扫一眼概览 → 扫描/纳管/升级。桌面窗口 ~1200×800 起。

## 核心信息（按优先级）
1. 有什么在等我决定（broken 链接 > 冲突 > 可升级 > 未纳管）
2. 扫描此刻在干什么（进度、可取消、不阻塞其他页面）
3. 系统健康事实（纳管数、落点数、投影生效数）——只报后端真有的数，不编活动流

## 情感基调
「精密工作台」：安静、可信、每个数字都有出处；扫描时是「机器在替我干活」的确定感，不是焦虑进度条。

## 输出格式与尺寸（三版必须统一）
- 单个 HTML 文件，1440×900 桌面窗口内渲染（macOS 窗口外壳：红黄绿三点 + 标题 Heron）
- 真实可交互：点「扫描本机」跑完整 4 状态时间线（模拟 ~12s 扫描：6 个 root、~52 个发现分批发泡）；深/浅主题切换按钮（复刻真实 app 的 data-theme 切换）
- 正文 ≥14px（app 内小字 11.5–13px 是既有系统，允许；对比度 ≥4.5:1）
- 三版同内容，只换设计；文件存 design-demos/dir-1.html … dir-3.html

## 设计系统（从 codebase 逐字抄，不许凭记忆重画）
来源：`ui/src/style.css`、`ui/src/App.vue`、`ui/src/views/OverviewView.vue`、`ui/src/views/DiscoverView.vue`

**双主题 tokens（:root[data-theme="dark"|"light"]）**
| token | dark | light |
|---|---|---|
| --bg | #090807 | #faf9f6 |
| --panel | #0f0e0c | #ffffff |
| --raised | #151310 | #f5f2ec |
| --line | rgba(246,240,230,.08) | #e8e3da |
| --line-strong | rgba(246,240,230,.16) | #d6cfc3 |
| --text | #efe8da | #211d18 |
| --dim | #c2bcb0 | #5c5650 |
| --faint | #a39b8c | #776f64 |
| --on-text | #181410 | #ffffff |
| --good | #b2c77a | #5c7a2e |
| --warn | #e3c08a | #8a6216 |
| --bad | #e47b66 | #a8452f |
| --accent | #9ea8ff | #4b4bc2 |
| --accent-weak | rgba(158,168,255,.14) | rgba(75,75,194,.1) |
| --shadow | 0 12px 32px rgba(0,0,0,.4) | 0 12px 32px rgba(33,29,24,.12) |

**字体**：--font-sans: "Manrope", -apple-system, "PingFang SC"…；--font-serif: "EB Garamond", "Songti SC", Georgia, serif（wordmark「Heron 苍鹭」用衬线）；--font-mono: "JetBrains Mono", ui-monospace…（路径、计数、tabular-nums）。root font-size 14px。
**圆角**：--radius 10px（卡片）、--radius-sm 7px（按钮/输入）。
**组件词汇**：.btn 高 30px padding 0 11px 12.5px/500；.btn.primary = var(--text) 底 + var(--bg) 字；.btn.sm 26px；.chip 22px 胶囊带 5px 圆点（good/warn/bad/neutral/accent，color-mix 12% 底）；.card = panel 底 + 1px line + radius；.card-head 12px 15px 下边线，h3 12.5px/600；.table th 11.5px/600 --dim raised 底；.stat 26px/700 letter-spacing -.02em tabular；.bar 4px 圆角条（i 为 --text，.bar.warn i 为 --bad）；.toast 右下 10px 8px 10px 11px 左边 3px 色条；图标 14px stroke 1.6 无填充。
**布局骨架（真实 app）**：shell grid 212px rail + 主区；rail: panel 底右边线，rail-head 34px seal + 衬线 wordmark，rail-search 32px，rail-group 12px/600 --dim，rail-item 30px（active 有左侧 2px accent 条），rail-count mono 11.5px；rail-foot 健康点 6px + Git 版本行；topbar 48px 下边线 crumbs 12.5px + 右侧按钮组（扫描本机 btn / 校验 btn.primary）；view padding 24px 18px 40px 28px。
**侧栏导航**（真实）：工作台/概览；收集·分发/我的 Skill(1)/发现(52 warn)/来源与市场；维护/更新/体检/项目/设置。rail 底部「52 待处理」「Git 2.50.1 本应用 v0.1.0」。
**客户端品牌色**（DiscoverView 真值）：agents #339af0, claude #e8590c, codex #0ca678, cursor #9c36b5, opencode #f08c00, workbuddy #e64980, antigravity #1098ad, trae #4263eb, qoder #d6336c, grok #e03131, cline #2f9e44, copilot #7048e8, heron #6c757d。
**发现卡片词汇**：glyph 34px 圆角方块 hsl(name hash 色相) 单字母 monogram；card 边框 line、hover raised；path mono 11.5px --dim 省略号。

## 真实内容（三版共用，不许 Lorem）
- 概览 lead：「收拢 → 分发 → 升级：下面只列现在真正需要你决定的事。」
- 接下来：`52 处 Skill 尚未纳管`（→发现）；若 broken>0 排最前（演示数据给 1 处坏链：claude 的 `pptx` 投影断链 → 体检）
- 流水线：已纳管 Skill 1 / 启用落点 6 / 可升级 0；生效中的投影 0/0；被压制/坏链 0/0；共 0 处投影 0%
- 落点状态表行（客户端/路径/已启用/状态）：heron(默认) ~/heron/skills、agents ~/.agents/skills、claude ~/.claude/skills、codex ~/.codex/skills、cursor ~/.cursor/skills、opencode ~/.config/opencode/skills
- 扫描 6 个 root（root_total=6）：~/.claude/skills、~/.agents/skills、~/.codex/skills、~/.cursor/skills、~/.config/opencode/skills、~/.trae/skills
- 扫描发现样例（skill 名 + 来源，演示用合理名，标注为样例）：huashu-design(claude)、pdf(codex)、lark-docs(agents)、pptx(claude)、wechat-mp(cursor)、logo-design(trae)、research(claude)、tdd(agents)、code-review(codex)、dynamic-ui(trae)、grilling(claude)、wizard(agents)…共 52 个，按 root 分批出现
- 顶栏按钮：扫描本机 / 校验；概览页头按钮：扫描本机 / 运行体检
- 完成 toast：「扫描完成 · 52 个新发现 · 2.8s」→ 跳发现页；truncated 变体：「结果不完整 · 已达 50 000 目录上限」→ 排除设置

## 已知约束
- 不弹模态；不遮挡内容；取消必须 ≤1 次额外点击；扫描期间其他页面照常可用
- 概览不许编造后端没有的数据（无编辑历史、无活动流）
- 外观家族：羊皮纸/暖墨/衬线 wordmark 是 Heron 的品牌，三版都在此系统内，差异必须落在**版式结构与信息组织**上，不是换色换字体

## Assumptions（用户未明说，按最佳判断补齐）
- 演示以浅色主题开场（用户截图为浅色），深色可切换
- 扫描时长模拟 ~12s（真实可能更久，原型不等待真实后端）
- 「增量流入」在概览与发现两处都可见（概览给轻量计数变化，发现给卡片流）
