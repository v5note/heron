# 发布与签名

Heron 用 **Tauri 内置的更新签名（minisign 密钥对）** 分发版本，**不依赖 Apple Developer ID，
也不依赖 Windows 代码签名证书**。

## 为什么这样选

| 方案 | 成本 | 作用 |
|---|---|---|
| Apple Developer Program | 每年 $99 | 公证，消除 Gatekeeper 警告 |
| Windows 代码签名证书 | 每年 $$$ | 消除 SmartScreen 警告 |
| **Tauri 更新签名**（本项目） | 免费 | 保证更新包**来源与完整性**，安装后自动更新不会被篡改 |

三者解决不同问题：前两个影响**首次安装**的信任提示，第三个保证**后续更新**的安全。
Heron 选择第三个：更新链路可信，首次安装需要用户手动放行。

### 实际影响

- **macOS**：首次打开提示"无法验证开发者"。用户右键 → 打开，或系统设置 → 隐私与安全性 →
  "仍要打开"。之后正常。
- **Windows**：SmartScreen 提示"Windows 已保护你的电脑"。点"更多信息" → "仍要运行"。
- **自动更新**：完全正常。更新包用私钥签名，客户端用内置公钥校验，**签名不符会拒绝安装**。

## 密钥

| 位置 | 内容 |
|---|---|
| `src-tauri/tauri.conf.json` → `plugins.updater.pubkey` | **公钥**，随源码公开 |
| GitHub Secret `TAURI_SIGNING_PRIVATE_KEY` | **私钥**全文（多行文本） |
| GitHub Secret `TAURI_SIGNING_PRIVATE_KEY_PASSWORD` | 私钥口令（本项目生成时为空，可留空） |

> 私钥**永远不要**提交到仓库。丢失私钥意味着已发布的客户端再也无法接收更新——只能让用户手动
> 重装。请离线备份。

### 生成一对新密钥

```bash
cargo tauri signer generate -w ~/.tauri/heron.key --password ""
```

把 `heron.key.pub` 的**全部内容**填进 `tauri.conf.json` 的 `pubkey`，
把 `heron.key` 的**全部内容**存为 GitHub Secret `TAURI_SIGNING_PRIVATE_KEY`。

## 发布流程

```bash
# 1. 改版本号（src-tauri/tauri.conf.json）
# 2. 写发布说明 releases/<version>.md
# 3. 打标签并推送
git tag v0.2.0
git push origin v0.2.0
```

标签推送后 CI 会自动：

1. 构建 macOS / Windows 包；
2. **检查私钥是否存在**——没有就失败（绝不发布未签名的包）；
3. 为更新载荷（`.app.tar.gz` / `.nsis.zip`）生成 `.sig`；
4. 校验每个载荷都有对应签名；
5. 生成 `latest.json`（含各平台下载地址与签名）；
6. 创建 GitHub Release 并上传全部产物。

客户端读取的地址是：

```text
https://github.com/v5note/heron/releases/latest/download/latest.json
```

## 校验签名（手工）

```bash
cargo tauri signer verify --signature Heron_x.y.z_x64-setup.nsis.zip.sig \
  --file Heron_x.y.z_x64-setup.nsis.zip --pubkey "$(python3 -c "import json;print(
json.load(open('src-tauri/tauri.conf.json'))['plugins']['updater']['pubkey'])")"
```

## 日后想加代码签名

若将来购买证书，只需在 `.github/workflows/ci.yml` 的 `release` 任务里补上
`APPLE_CERTIFICATE` / `APPLE_SIGNING_IDENTITY` / `APPLE_ID` 等环境变量，
Tauri 会自动完成签名与公证。**更新签名的配置不需要任何改动**，两者互相独立。
