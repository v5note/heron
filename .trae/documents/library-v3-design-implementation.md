# 按 library-v3.html 设计开发「我的 Skill」页

## Context

用户已批准方向①「运维控制台」并迭代出高保真原型 `design-demos/library-v3.html`（含最新的「列表/网格」双视图）。现要求把该设计落地到真实 app：「我的 Skill按照library-v3.html的设计来开发」。目标文件是 Tauri+Vue3 前端 `ui/` 中的 `LibraryView.vue`，**不是**移植原型 HTML，而是用真实组件、token、i18n、store 复刻设计的每个决策点。原型设置页不落地（真实 SettingsView 已有主题 UI）；原型「示例」标签不落地（真实数据全为真）。

已核实的关键现状：
- `ui/src/style.css` 只有 `.matrix .dot`（L289-294，4 态），**无全局 `.dot`**，缺 `restart_required` 样式；`.table tbody tr.selected/:hover` 规则（L374-378）要求 `.striped`，而 `LibraryView.vue` 的表格没带 → 现勾选高亮不生效。
- `visibilityBySlug`（`stores/app.ts`）只返回 enabled targets；路径权威来源是 `VisibilityReport.path`，显示缩写用 `displayPath(path, store.home)`（`utils/paths.ts`）。
- origin 实际取值：`store / adopted / legacy-command`；`AppIcon` 已有 `grid` 图标。
- 弹层先例：`DiscoverView.vue` `.client-menu`（外部 mousedown 关闭 + Esc + role=listbox）；tooltip 项目惯例 = 原生 `title`。
- 校验链：`pnpm --dir ui i18n:check` / `typecheck` / `build`；`dev` 端口 5183。无组件测试框架。

## 实现决策（定案）

1. **颜色**：全部走真实 token（`--panel/--good/--warn/--bad/--accent/--raised/--line-strong/--faint` + `color-mix(var(--*))`），不搬任何原型 hex。
2. **视图切换**：组件内 `ref<ViewMode>`，持久化键 **`heron.libraryView`**（值 `list|grid`，读取白名单校验、非法回落 list），不用原型的 `heron.view`（避免与未来的其它 view 撞名）。
3. **tooltip**：原生 `title`（`落点名 · 状态文案`），不自绘。
4. **弹层**：照 DiscoverView 模式在 LibraryView 内自建 `.pop`，不抽公共组件。
5. **健康读数条口径**：分母 = 该 target 有 verify 报告行且 slug 仍在 `store.skills` 的技能数（=真正放过链接数），分子 = 其中 `visible` 数；需处理 = shadowed+restart_required+broken；全量计算不随搜索/筛选变化；`targets.length===0` 整块不渲染；列数 `auto-fit/minmax(160px,1fr)`。
6. **`.seg`/`.kv` 留在 scoped**（唯一使用者是本视图，不上提全局）。
7. **不动**：`visibilityBySlug` 签名、`heron.theme` 迁移逻辑、DiscoverView、store。

## 分步计划

### Step 1 — `ui/src/style.css`：点词汇上提 + 补两态
- 新增全局 `.dot`（10px 实心基线 + 5 个状态类），`.matrix .dot` 只管 8px 密度、`.dot.*` 只管颜色（注意去掉 `.matrix .dot` 的 background 以免顺序取胜）。
- 补 `.dot.restart_required`：`--warn` inset 描边 + `::after` 中心点（`inset:25%`，两档密度自适应）；注释注明 `classify()` 尚不返回此态，属前瞻补齐。
- 注释只用 `/* */`。

### Step 2 — i18n 双文件（`ui/src/i18n/locales/zh-CN.json` + `en-US.json`）
- 新增 `library.*`：`viewList/viewGrid/projectMenu/projectAll/selectedN/selectHint/liveCount/healthTitle/healthNeedFix/healthUnscanned/healthAllGood/healthEmpty/noMatch/legendNote/originStore/originAdopted/originLegacy`。
- 改值：`filterIssues` 带 `{n}` 计数；删除 `library.legend`（图例改为状态样本点 + 现有 `visibility.*` 标签生成）。
- 抽屉区块标题复用现有 `library.targets`。
- 验证：`pnpm --dir ui i18n:check`。

### Step 3 — `LibraryView.vue` `<script setup>` 增量
- `ViewMode` + `VIEW_KEY="heron.libraryView"` + `view` ref + `setView()`（赋值即写 localStorage，不用 watch）。
- `ORIGIN_TONE`（store→neutral / adopted→accent / legacy-command→warn，未知值回落 mono 原始串）+ `originTone()/originLabel()`。
- `issueTotal` 复用与 `rows` 相同的谓词，保证 seg 数字与筛选结果一致。
- 弹层：`popOpen` + `popBox` + onMounted/onBeforeUnmount 外部关闭（抄 DiscoverView）；`projectSelected` 增加「全部落点」路径；开抽屉前强制关弹层。
- `targetHealth` computed：单次遍历 `store.visibility` 建 Map，输出每 target `{total, visible, needFix, unscanned}`。
- `drawerRows` computed：`visibilityBySlug` join 报告行取 `path`（显示走 `displayPath`，title 全路径）。
- 删模板中恒真的 `targets.some(...)` 过滤；不引入任何 `sample` 概念。

### Step 4 — `LibraryView.vue` `<template>`
- PageHeader `#actions`：选中 hint + 「投影到…」弹层（v-for enabled targets，右侧 `displayPath`；末项全部落点）。
- 工具条：搜索 → 过滤 seg（全部 n / 有问题 n）→ spacer → 视图 seg（列表/网格）；**删除原 6 枚「投影到 X」按钮**。
- 列表视图 `.lib-card>table`：来源列换 chip；目标列 `title` + 10px 全局 `.dot`；勾选/hover 靠新增 scoped 规则（不加 striped）。
- 网格视图 `.grid>.g-card`：原生 checkbox + slug + 来源 chip、描述 2 行 clamp、`g-matrix`（3 列 dot+落点名，title tooltip）。
- 空态三分：无技能 / 过滤无结果（`noMatch`，两视图都有）/ 无启用落点。
- 健康读数条 `.hstrip`：每落点一格（分数 + 六段 `.hbar` + 副行）。
- 图例：5 个状态点样本 + `visibility.*` + `legendNote`。
- 抽屉：头部 `liveCount`；per-target `.trow`（落点/路径/StateChip/投影切换按钮）；kv 来源用 `originLabel`；路径单元 `unicode-bidi:plaintext`。

### Step 5 — `LibraryView.vue` `<style scoped>`
新增：`.tcol`、`.lib-card`（overflow-x + min-width:720px）、hover/selected 行规则、`.grid`（auto-fill minmax(280px,1fr)）、`.g-card/.g-desc/.g-matrix/.g-t`、`.pop/.pop-menu/.pop-item`（token 抄 DiscoverView）、`.hstrip/.hcell/.hbar` 六段、`.legend` 重写、`.trow/.tp`。保留现有 `.seg/.kv`。

### Step 6 — 验证
1. `pnpm --dir ui i18n:check` && `typecheck` && `build` 全绿。
2. `pnpm --dir ui dev`（5183）人工检查：
   - 4 主题 × 明暗 = 8 组合遍历（restart_required 点、hbar 分段、来源 chip 对比度）；
   - 视图切换 + 刷新记忆 + 手改非法值回落 list；
   - 网格勾选→弹层批量投影→toast；点卡片开抽屉；
   - 弹层外部点击/Esc 关闭，与抽屉不冲突；
   - 三空态；`--` 表格与网格对同一技能读数一致；
   - 回归 OverviewView/ProjectsView 的 `.matrix` 点未变形。
3. 前端无浏览器测试框架，验证以 typecheck+build+dev 人工检查点为准。

## 改动文件
- `ui/src/style.css`（点样式上提 + restart_required）
- `ui/src/i18n/locales/zh-CN.json`、`en-US.json`
- `ui/src/views/LibraryView.vue`（主体）
- 只读参考：`design-demos/library-v3.html`、`ui/src/views/DiscoverView.vue`

## 风险
- 混入原型 hex → 硬约束只允许 `var(--*)`/`color-mix`，8 组合逐一过。
- `.dot` 上提影响其它视图 → `.matrix .dot` 保留尺寸规则，回归两视图。
- `restart_required` 线上暂不可达（`classify()` 不返回）→ 如实注明前瞻，不造假数据提交。
- 健康条分母口径 → 注释 + 文案写明「放进该落点的链接里有多少真生效」。
