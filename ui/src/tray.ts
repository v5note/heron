/**
 * The tray / status-bar icon.
 *
 * Only the shell can create one (there is no tray module in `@tauri-apps/api`), but the shell cannot
 * name its rows either: the labels are UI copy, and the language lives in this webview. So the split
 * is the same one the menu bar makes — the UI owns the text and sends it down through `tray_install`,
 * the shell owns the icon and the two rows it must answer itself (`show`, `quit`).
 *
 * Every other row comes back as a `tray://action` id and runs the **same** function the menu bar
 * runs, from `menu.ts`. One id, one behaviour, two entry points.
 *
 * Calling `setupTray` again — which is what a language switch does — rebuilds the menu in place; it
 * never adds a second icon.
 */

import { watch } from "vue";
import { api, onEvent, type TrayItem } from "./api";
import { actions } from "./menu";
import { i18n } from "./i18n";

type Global = { locale: { value: string }; t: (key: string) => string };

const messages = i18n.global as unknown as Global;

/** The menu, in the order it is drawn. */
function rows(): TrayItem[] {
  return [
    { id: "show", text: messages.t("tray.show") },
    { id: "rescan", text: messages.t("menu.rescan") },
    { id: "settings", text: messages.t("menu.settings") },
    { separator: true },
    // The only way out once closing the window merely hides it.
    { id: "quit", text: messages.t("tray.quit") },
  ];
}

function refresh(): void {
  // Worth logging, worth nothing else: the tray is an extra way in, and the window, the menu bar
  // and every button in the UI keep working without it.
  void api.trayInstall(rows()).catch((e) => console.warn("tray:", e));
}

let installed = false;

export function setupTray(): void {
  if (installed) return;
  installed = true;
  onEvent<string>("tray://action", (id) => actions[id]?.());
  refresh();
  watch(() => messages.locale.value, refresh);
}
