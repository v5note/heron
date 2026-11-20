# src-tauri/icons

Heron 应用图标，由 `scripts/make-icons.py` 从 `docs/icons/white-bg-black-silhouette/icon-master.png`（本次提供的苍鹭原图）生成。
**不要手工替换这里的文件**：改图请替换母图后重跑脚本。

```bash
python3 scripts/make-icons.py
```

## macOS

- 画布 1024 × 1024，圆角底板 824 × 824（四周透明留白 100 px，即 1024 的 9.8%），
  与 v5notes client2 的 `app-icon-source.png`（103 px 留白）一致。
- 连续超椭圆（squircle）圆角，圆角系数 0.400，让底板边角更接近截图中的相邻图标。
- 苍鹭本体（黑色剪影、金橙色喙和眼睛，留白处的眼瞳和羽线保留为白色）占底板宽度
 约 64%、高度不超过 84%，在白色底板内留出更多空间。
- `icon.icns` 内含 16 / 32 / 64 / 128 / 256 / 512 / 1024 px 七个尺寸。

## Windows

- `icon.ico` 含 16 / 24 / 32 / 48 / 64 / 128 / 256 px 七个尺寸，NSIS 安装器与 exe 均可用。
- `icon-windows.png` 与 `Square*Logo.png`、`StoreLogo.png` 为满幅底板（不留透明边，
  系统自身会在磁贴内内缩 4 px，同 v5notes client2 的 `icon-windows.png`），供开始菜单、
  任务栏与 MSIX 磁贴使用。

## 文件

| 文件 | 用途 |
| --- | --- |
| `icon-master.png` | macOS 圆角底板 1024 px 母图 |
| `windows-master.png` | Windows 满幅白底 1024 px 母图 |
| `icon.icns` | macOS 应用图标（`tauri.conf.json` 引用） |
| `icon.ico` | Windows 应用图标（`tauri.conf.json` 引用） |
| `32x32.png`、`64x64.png`、`128x128.png`、`128x128@2x.png` | Tauri 打包用 PNG（`tauri.conf.json` 引用） |
| `icon.png`、`16x16.png`、`24x24.png`、`48x48.png`、`256x256.png`、`512x512.png` | 额外 PNG 尺寸 |
| `icon-windows.png`、`Square*Logo.png`、`StoreLogo.png` | Windows 磁贴 / 商店素材 |
