# 微信公众号内容立项卡（Brief）— Heron 苍鹭

## 1. 主题与一句话定位
- **项目名称**：Heron 苍鹭（v0.1.0 · MIT 开源）
- **一句话定位**：把散落在各处的 Agent Skill 收拢成一份受 Git 版本管理的单一真实源，按需投影到 13 款 AI 客户端，用三方合并安全升级。
- **核心标语**：苍鹭静立一处，注视整片水面。
- **官网与仓库**：
  - 官网：https://heron.v5ai.xin
  - GitHub：https://github.com/v5note/heron

## 2. 目标读者
- **V1 深色版（开发者 / 技术决策者）**：同时使用 Claude Code、Cursor、Codex、Trae、Copilot 等多款 AI 编程工具的工程师与技术团队负责人，关注架构洁癖、Git 版本控制、安全边界与团队锁文件。
- **V2 浅色版（AI 效率玩家 / 泛技术读者）**：日常在不同 AI 工具间切换、收藏了大量 Skill 却经常找不到或不敢更新的开发者与产品/效率用户，关注直观界面、三步上手与省心体验。

## 3. 核心卖点（6 条，每条有代码/截图实证）
1. **单一真实源 + 哈希去重**：`heron scan` 扫全磁盘，`heron adopt` 先备份再 Blake3 去重收拢至 `~/.heron/skills/`，原位替换为受管链接。
2. **13 款 AI 客户端诚实探测**：内置 Claude Code、Codex、Cursor、Trae/Trae CN、Qoder、WorkBuddy、Cline、GitHub Copilot、Grok Build、Antigravity、opencode、`.agents`，逐落点开关并真实校验生效状态（生效中/被压制/坏链/未扫描）。
3. **4 种跨平台投影模式**：macOS/Linux 用 `symlink`，Windows 用免管理员提权的 `junction`，兼容特殊扫描器用 `pointer` 代理文件，兜底用带 `E_DRIFTED` 漂移防护的 `copy`。
4. **Per-Skill Git 仓库 + 三方合并升级**：每个 Skill 独立 Git 仓库，基于 `git merge-tree` 内存级三方合并，升级上游同时保留本地提示词微调，冲突可视化逐文件合并，随时 `heron rollback`。
5. **6 大市场统一检索 + 团队锁文件**：一框直搜 `skills.sh` / `ClawHub` 直装，引导 `Qoder` / `SkillHub` / `ModelScope` / `SkillsMP`；导出无绝对路径的 `heron.lock` 一行复现团队环境。
6. **Rust + Tauri 2 原生安全契约**：5–15 MB 零运行时依赖，GUI 与 CLI 同构内核，先备份再动文件，拒绝隐式覆盖，只读体检。

## 4. 栏目与品牌
- **品牌名**：Heron 苍鹭
- **Logo 缩写**：H
- **品牌网站**：heron.v5ai.xin

## 5. 双版本风格路由（一深一浅 · 结构不同）

| 维度 | V1 深色技术版（`v1-forest`） | V2 浅色工具版（`v2-bento`） |
|---|---|---|
| **明暗与主题** | 深色系 · `forest`（林墨深绿，呼应官网默认主题） | 浅色系 · `bento`（暖米纸底 + 橙红强调） |
| **首图骨架** | `dev-dark`（深色首页真截图 + 2×2 能力格 + 先了解条） | `tool-brief`（浅色我的 Skill 真截图 + 底部双 pill） |
| **贴图主视觉差异** | 代码痛点窗 + 闭环流 + 抽屉真截图 + 分层架构 + 新旧对比 | Bento 大小格能力墙 + 菱形判断流程图 + 发现/市场真截图 + 安全底线清单 |
| **文章骨架** | A · 开源项目推荐（01/02 编号，硬核架构 + 参数表 + 边界） | H/B · 桌面端体验与上手指南（一/二 编号，场景切入 + 三步教程 + 避坑） |
| **二维码落点** | https://heron.v5ai.xin（官网） | https://github.com/v5note/heron（GitHub） |
