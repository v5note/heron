# 构建任务：scan-overview-v2.html（在已选方向①上迭代）

先完整读这四个文件（按序）：
1. `design-demos/spec.md` —— 内容数据、token 表、组件词汇、交互契约（唯一事实来源）
2. `design-demos/direction-approved.md` —— 已选方向①与迭代豁免记录
3. `design-demos/dir-1.html` —— 方向①的原型本体：复用它的 Vue3-CDN+gsap 技术栈、窗口骨架（rail/topbar/toast/drawer）、ICON 图标库、CSS 命名（--h-* / .h-*）。整体读，不要跳
4. `design-demos/thumbs.html` + `directions.md` —— 结构探索记录（最佳结构已定：Rams 仪器读数 + Linear 实时流入，吸收进①的语言，不换外观家族）

产出：`/Users/jingyuan-mb01/javaspace/REA/heron/design-demos/scan-overview-v2.html`（单文件，fill-viewport 桌面窗口，1440×900 设计基准）。

## 截图修正（以 18:52 用户截图为准，覆盖 spec 中冲突处）
- 面包屑是「苍鹭 / 概览」（app.short=苍鹭），不是 Heron
- 概览页头按钮：`扫描本机` 是 primary（深墨底白字），`运行体检` 次要；顶栏：`扫描本机` 次要、`校验` primary
- 「接下来」行：中性 chip「待处理」+「52 处 Skill 尚未纳管」+ 右侧小按钮「发现」
- 「流水线」卡头右侧文案是「刷新」；三读数= 1 已纳管 Skill / 6 启用落点 / 0 可升级（0 用 warn 色）
- 落点状态表含重复客户端行（真实数据）：heron(默认) ~/.heron/skills、agents×2、claude×2、codex×2、cursor、opencode…；「已启用」列全 0；「状态」列是单个空心圆点（not_scanned）

## 概览页新结构（自上而下）
1. PageHeader：概览 + lead（spec 原句）+ 两个动作按钮（扫描按钮=胶囊状态机，见下）
2. **扫描状态行**（新增，mono 12.5px --faint，页头下方一行，细分隔线）：空闲=`上次扫描 42 秒前 · 遍历 6 个根 · 发现 52 处 · 已纳管 1`；扫描中=`阶段 · 遍历中 → ~/.codex/skills · root 2/6 · 已发现 17`+行内`取消`链接；truncated 时该行尾追加 warn 小字`结果不完整`
3. 双栏 grid（左 1.35fr / 右 1fr）：
   - 左卡「决定队列」（替代“接下来”）：按严重度排序的行（坏链行最前：bad chip+`claude · pptx 投影断链`+按钮`体检`；待纳管行：warn chip+计数**扫描中实时跳动**+按钮`发现`）；空态`一切干净`
   - 右卡「系统读数」（替代“流水线”）：三枚大 tabular 数字（待决定 52 / 启用落点 6 / 已纳管 1；扫描中第一枚变实时发现计数，变化时 --accent 短暂脉冲）；其下 **root 刻度计**：6 段小格，完成 root 点亮 good、当前 root 流光、未开始 line；再下分隔线 + 原「生效中的投影 0/0、被压制/坏链 0/0、共 0 处投影 0%」条块（保留 .bar 词汇）
4. 「落点状态」表全宽保留（含重复行与空心圆点）

## 扫描交互状态机（核心，必须真实可点）
- 顶栏与概览两处扫描按钮共享一个 scanState（Vue 响应式）；按钮固定 min-width≈150px 不抖动
- 四态：`扫描本机` → `⟳ 扫描中 2/6`（图标 2s 慢旋，prefers-reduced-motion 停转；按钮内嵌 6 微刻度）→ hover/再点=`点击取消 · 2/6`（warn 描边）→ 完成短暂`✓ 52 发现` 1.6s 后复位
- 模拟时间线 ~12s：准备 0.8s → 6 个 root 各 ~1.7s（不均匀、root 内 indeterminate）→ 入库 0.6s → 汇总 0.4s；发现按 root 分批（9/5/3/8/6/21=52）每 ~300ms 冒一小批
- **顶栏下 2px 全局进度轨**：topbar 底边线下，扫描中 accent indeterminate 流光，跨页面存活；完成瞬间变 good 全宽 0.4s 后淡出；取消立即消失
- **增量流入**：扫描中切到「发现」页，卡片分批 prepend 进网格（stagger 淡入+上移 6px），页头 chip`+N 新发现`；侧栏「发现」计数实时跳动+轻微弹一下
- 取消：toast`扫描已取消 · 已发现 17 处保留`，已流入数据保留，状态复位；扫描中再点=取消/聚焦，永不排队第二个任务
- 完成 toast（右下，复用 .h-toast 词汇）：`扫描完成 · 52 个新发现 · 2.8s`+按钮`查看`→跳发现页；按 `T` 键切换 truncated 变体：bad 色条 toast`结果不完整 · 已达 50 000 目录上限`+按钮`排除设置`→设置页
- 扫描期间其他页面照常可点（证明不阻塞）

## 演示控制（截图与自查用，必须实现）
URL 参数：`?theme=light|dark`（默认 dark=方向①；light 必须同样正确）、`?scan=auto`（载入即开扫）、`?at=SECONDS`（快进到时间线某秒）、`?view=overview|discover`。Esc=取消扫描。

## 红线
- 只从 dir-1 的 token/组件词汇生长，不引入新外观家族；不编造数据（内容一律取 spec）
- 图标全部复用 dir-1 的 stroke SVG ICON 库（新增可用 refresh/cancel/check/alert），无 emoji
- 正文 ≥14px（既有 11.5–13px 小字系统允许）；深浅两主题对比度 ≥4.5:1
- 单文件；>1000 行也放一个文件（原型惯例）

## 自检与交付（必做）
1. `cat /tmp/pw-install.log` 确认 chromium 已装好（后台任务在装；没好就等或重跑 `npx playwright install chromium`）
2. 截图到 `design-demos/screens/`（1440×900）：v2-dark-idle / v2-dark-scanning（?scan=auto&at=5）/ v2-dark-cancel-hover（hover 态可用 at 后手动 dispatch mouseenter 或截 running 态即可）/ v2-dark-done（at=12.5 看 toast）/ v2-dark-discover-stream（view=discover&at=8）/ v2-light-overview / v2-light-scanning
3. 逐张 Read 截图看：溢出、错位、宽度抖动、对比度、圆角/色值是否走 token；发现问题改完再截，直到干净
4. `python3 /Users/jingyuan-mb01/.agents/skills/huashu-design/scripts/design_lint.py check scan-overview-v2.html` 红灯必修
5. 汇报：文件路径、截图清单、lint 结果、你主动做出的设计取舍（≤10 条）
