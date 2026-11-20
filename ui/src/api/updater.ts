// Application self-update (distinct from skill upgrades).
//
// Signed with the minisign key pair whose public half is embedded in tauri.conf.json, so an
// artifact that was not produced by a holder of the private key is rejected by the plugin
// before it is ever installed.

import { check } from "@tauri-apps/plugin-updater";
import { relaunch } from "@tauri-apps/plugin-process";

export interface UpdateInfo {
  version: string;
  currentVersion: string;
  notes: string | null;
}

/** Returns metadata when a newer release exists, or null. Never downloads anything. */
export async function checkForUpdate(): Promise<UpdateInfo | null> {
  const update = await check();
  if (!update) return null;
  return {
    version: update.version,
    currentVersion: update.currentVersion,
    notes: update.body ?? null,
  };
}

/** Download, verify the signature and install, then restart into the new version. */
export async function installUpdate(onProgress?: (downloaded: number, total: number | null) => void) {
  const update = await check();
  if (!update) return false;
  let downloaded = 0;
  let total: number | null = null;
  await update.downloadAndInstall((event) => {
    if (event.event === "Started") {
      total = event.data.contentLength ?? null;
    } else if (event.event === "Progress") {
      downloaded += event.data.chunkLength;
      onProgress?.(downloaded, total);
    } else if (event.event === "Finished") {
      onProgress?.(downloaded, total);
    }
  });
  await relaunch();
  return true;
}
