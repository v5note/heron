# 方向选定记录（Gate)

## 展示了哪几版

三方向初稿（每个方向浅/深两套主题），截图见 `screens/`：

- ① 运维控制台：深色优先（`dir-1.html`），浅色为暖纸版（`dir-1-light.html`）
- ② 编辑部：浅色优先（`dir-2.html`），深色 `dir-2-dark.html`（初稿「田中几何册」因与 ③ 撞家族已撤）
- ③ 信息密集台：浅色优先（`dir-3.html`），深色 `dir-3-dark.html`

## 用户的选择（原话）

> ① 运维控制台（深色优先，浅色是暖纸版）

## 落地约定

- 骨架按 ① 落地：212px 侧栏分三段（工作台 / 收集 · 分发 / 维护）+ 顶栏面包屑与主操作 + 内容区；状态 4 列圆点矩阵；侧栏底健康条（git / 待处理 / 主题切换）。
- 默认主题 **深色**（`--h-bg:#090807` 系，暖中性深底 + 单一青绿点缀 `--h-vit:#9EA8FF`），浅色为暖纸版（`#FAF9F6` + 同骨架）。
- 主题写入 `localStorage['heron.theme']`，设置页「主题」项复用 i18n 已有的 `settings.theme` 键。

## 迭代记录（2026-11-21）：扫描进度交互 + 概览页改版

用户原话：「以上方案没有问题。先按照这个方案设计交互UI，同时把概览页也重新设计一下。」

豁免依据：同一项目内已选定方向（① 运维控制台）后的部分修改（改交互组件 + 改一个页面），按 skill 规则不重做三方向门，从已选版本出发。
已读上一轮选择与本条用户原话后才动手；结构探索（非新方向提案）见 `directions.md` 与 `thumbs.html`，最佳结构已吸收进 ① 的语言：Rams 仪器读数 + Linear 实时流入，不换外观家族。
扫描交互契约以已批准方案为准（按钮变形胶囊 / 顶栏 2px 轨 / 增量流入 / 4 状态 / 可操作 toast / truncated 明示），产出物：`scan-overview-v2.html`。

## 迭代记录（2026-11-21 · 我的 Skill 页 v3）

用户原话：「Use Skill: huashu-design 设计「我的Skill」页面」（附 3 张现实现截图：表格、图例、详情抽屉）。

豁免依据：同一项目内已选定方向（① 运维控制台）后的页面级迭代，不重做三方向门，从 ① 的语言出发。
产出物：`library-v3.html`（深色优先 + 暖纸浅色可切换，浏览器实测：表格/抽屉/筛选/批量投影/取消投影/主题切换全 PASS，无 console 报错）。
页面级信息架构修正（相对截图中的现实现）：① 工具条 6 个「投影到 X」批量按钮收进「投影到…」弹层（原来长得像筛选）；② 表格下方新增「落点健康」读数条，把单行数据时的空白画布变成每列的仪器读数；③ 补 `restart_required` 第 5 种点样式（warn 描边 + 中心点，现实现回落灰色）；④「状态」列语义错位改为「来源」chip，实体状态进抽屉 kv；⑤ 抽屉路径去掉 direction:rtl 反排缺陷。
数据诚实：code-review 行为真实数据（adopted/active/6 落点未扫描），其余 4 行标「示例」，落地时删除。

追加（同日 · 用户指令「添加选项」+ 真实 app 设置页截图）：`library-v3.html` 内新增设置视图——主题（深色/浅色 segmented）/ 通用（默认投影模式、3 开关、语言）/ 数据与路径（3 行路径 + 打开）/ 客户端（6 落点开关 + 启用计数），选项与截图一一对应；侧栏「我的 Skill」「设置」互切，其余导航 toast 指向 dir-1.html。浏览器实测 6 步全 PASS、console 无报错；lint 红灯 0。

## 迭代记录（2026-11-22 · 列表/网格双视图）

用户原话：「我的 Skill添加「列表」和「网格」支持」。

豁免依据：同一页面（`library-v3.html`）的增量功能，不换方向、不换家族。
产出物：工具条右端新增视图 seg（列表 / 网格），选择持久化到 `localStorage['heron.libraryView']`，刷新记忆；网格卡片 = 勾选框 + 名称 + 来源 chip + 描述 2 行 clamp + 落点圆点（dot + 落点名），与列表共享选择集、批量投影弹层、筛选与搜索、详情抽屉。lint 红灯 0，黄灯 9（数据类等宽字体，属数据诚实标记，保留）。

## 设计落地（2026-11-22 · LibraryView.vue 真实实现）

用户原话：「我的 Skill按照library-v3.html的设计来开发」。

按已批准计划（`.trae/documents/library-v3-design-implementation.md`）将 `library-v3.html` 的设计落地到真实 app（Tauri + Vue 3），改动：
- `ui/src/style.css`：`.dot` 上提为全局词汇并补 `restart_required` 第 5 态；`.matrix .dot` 只管 8px 密度。
- `ui/src/i18n/locales/{zh-CN,en-US}.json`：`library.*` 新增 18 键（视图切换/投影弹层/批量选择/健康条/图例/来源 chip 等），删除 `library.legend`（图例改由状态点样本生成）。
- `ui/src/views/LibraryView.vue`：整体重写——列表/网格双视图（`heron.libraryView` 持久化）、「投影到…」弹层与批量投影、落点健康读数条（分母改为该落点有报告行的托管技能数）、图例、抽屉（权威路径取 `VisibilityReport.path`）、空态三分。全部样式仅用 `var(--*)` token，适配 4 主题 × 深浅双模式。
- `ui/src/components/AppIcon.vue`：补 `list` 图标。

验证：`i18n:check` 331 键 OK、`typecheck`、`build`、`ipc:check` 27 命令 OK；经临时 mock 页（`ui/mock-dev.html`，验证后已删除）浏览器实测 6 项全 PASS（默认列表、网格+持久化、批量投影+toast、抽屉计数与互斥、筛选/搜索/空态、console 0 error）。原型中「示例」标记数据未在真实代码中保留，真实数据全部来自 store。
