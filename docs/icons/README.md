# Heron 应用图标

苍鹭侧影应用图标：身体以黑、白和冷灰剪影表现，头部保留金橙色喙与眼睛；深青色底板采用圆角造型，外沿透明。

## 文件

- `icon-master.png`：1024 × 1024 RGBA 高分辨率母图。
- `icon.icns`：macOS 应用图标。
- `icon.ico`：Windows 多尺寸图标。
- `32x32.png`、`128x128.png`、`128x128@2x.png`、`icon.png`：Tauri/Linux 常用 PNG 图标。
- `16x16.png`、`24x24.png`、`48x48.png`、`64x64.png`、`256x256.png`、`512x512.png`：额外 PNG 尺寸。

`src-tauri/tauri.conf.json` 已指向本目录中的图标文件，Tauri 打包时会分别读取 macOS、Windows 和 PNG 图标。
