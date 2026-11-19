# ADR-0009: 市场适配分两级；`heron.lock` 只记录"什么"与"从哪来"

- 状态：已采纳（2026-11-19）
- 相关：设计 §9.5 / §7.5、实现文档 §6.3

## 背景（实测）

- **ClawHub** 的 `/api/v1/search` 可用，但返回的条目**没有 Git 源**（`install.sourceUrl` 与
  `links.source` 均为 null）——它是托管平台，内容不在 Git 里。
- **skills.sh** 没有 JSON API，但 `sitemap-skills-*.xml`（870 KB/分片）可解析出
  `/<owner>/<repo>/<skill>`，直接映射到 `github.com/<owner>/<repo>` + `skills/<skill>`。
  偶发 TLS 握手失败，需要重试。
- qoder / skillhub.cn / skillsmp 无可用的机器接口（纯前端渲染或反爬）。

## 决策

1. 适配器分两级：**searchable**（skills.sh、ClawHub，能产出安装计划）与
   **browser only**（其余三家，产出深链，引导用户把 Git 地址粘回来）。
2. 只有能产出 `InstallPlan::Git` 的条目在 CLI 中标 `+`，其余标 `~`——界面不假装能一键安装。
3. HTTP 层对瞬时 TLS/连接错误重试 3 次（指数退避起点 250 ms）；失败降级为
   `E_MARKET_UNAVAILABLE`，不阻断其它命令。
4. `heron.lock` 记录 `source / ref / subpath / commit / version / contentHash`，**不记录绝对路径**；
   导入时逐条比对内容哈希，不一致即报 drift（`ContentChanged`），缺失即 `Missing`。

## 后果

- 五个市场都能从 GUI/CLI 到达，但"能装"与"能看"被明确区分；
- 团队协作以 `heron.lock` 为契约，跨机器/跨 OS 可复现，且能发现本地漂移；
- 新增市场只需实现一个 `MarketAdapter`，无需改调用方。
