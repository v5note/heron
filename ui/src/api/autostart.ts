// 开机启动（登录时自动运行 Heron）。
//
// 状态存在操作系统里（macOS 的 LaunchAgent、Windows 的注册表、Linux 的 autostart desktop 文件），
// 不是 localStorage 的镜像，所以开关的值一律来自 `isEnabled()`，写入后也用它确认实际结果。
//
// 和 updater.ts 一样走插件自己的命令（`plugin:autostart|*`），不经过 `call()`，因此不在
// ipc:check 的对照表里。

import { disable, enable, isEnabled } from "@tauri-apps/plugin-autostart";

/**
 * What the OS currently says. A browser dev session has no Rust host, and a machine where the app
 * was never registered answers `false` — the two are told apart here only by the throw, which the
 * caller swallows at boot and reports on a click.
 */
export async function readAutostart(): Promise<boolean> {
  return await isEnabled();
}

/** Ask the OS to turn it on/off, then read back what it actually ended up as. */
export async function writeAutostart(on: boolean): Promise<boolean> {
  if (on) await enable();
  else await disable();
  return await isEnabled();
}
