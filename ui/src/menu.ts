/**
 * The native menu bar.
 *
 * Built on this side of the IPC because its labels are UI copy: the titles come from `menu.*` in
 * vue-i18n, so switching 语言 in Settings rebuilds the bar in the same language as the rest of the
 * app. The shell cannot do that — the choice lives in the webview's localStorage — and a menu built
 * there would drift one language behind the interface. Items the OS already knows (copy, paste,
 * minimize, quit…) stay predefined and keep their system-localized names.
 *
 * **Windows gets no menu bar.** There the menu is drawn inside the window, above the client area,
 * where it competes with the topbar that already carries 扫描/校验; macOS uses the app menu and
 * Linux the window menu instead. Tauri installs no menu on its own off macOS, so "skip Windows"
 * really does mean "nothing appears".
 *
 * Nothing here implements a feature: every action delegates to the same store action, route or
 * updater call the page itself uses.
 */

import { watch } from "vue";
import {
  Menu,
  type MenuItemOptions,
  type MenuOptions,
  type PredefinedMenuItemOptions,
} from "@tauri-apps/api/menu";
import { openUrl } from "@tauri-apps/plugin-opener";
import pkg from "../package.json";
import { checkForUpdate } from "./api/updater";
import { i18n } from "./i18n";
import { router } from "./router";
import { useAppStore } from "./stores/app";

const appName = "Heron";
/** Same feed the updater is configured against in tauri.conf.json. */
const homepage = "https://github.com/v5note/heron";

type Global = {
  locale: { value: string };
  t: (key: string, named: Record<string, string | number>) => string;
};

const messages = i18n.global as unknown as Global;

const label = (key: string, named: Record<string, string | number> = {}) => messages.t(key, named);

/** Menu item id → what it does. Shared with the tray: the same id must do the same thing. */
export const actions: Record<string, () => void> = {
  // The scan belongs to the store, not to the page on screen, so the menu can start it from
  // anywhere — which is the whole reason it is a menu item rather than a second button.
  rescan: () => void useAppStore().startScan(),
  settings: () => void router.push("/settings"),
  theme: () => useAppStore().toggleTheme(),
  website: () => void openUrl(homepage),
  checkUpdate: async () => {
    const store = useAppStore();
    try {
      const update = await checkForUpdate();
      if (!update) {
        store.toast("ok", label("settings.upToVersion", { version: pkg.version }));
        return;
      }
      // The menu finds the release; installing it stays the Settings page's job, so the progress bar
      // and the relaunch confirmation are not duplicated here. A failed check is a warn, not an
      // error — same as the page, because in a dev build there is simply no update feed.
      store.toast("warn", `${label("settings.updateAvailable")} ${update.version}`, {
        action: { label: label("nav.settings"), to: "/settings" },
        ms: 12_000,
      });
    } catch (e) {
      console.error(e);
      store.toast("warn", label("settings.updateFailed"));
    }
  },
};

/**
 * The three helpers below are annotated on purpose. Left to inference an object literal inside a
 * big array literal widens `item` to `string`, and the bar then no longer matches
 * `MenuOptions["items"]`, whose predefined entries accept only the known kinds.
 */
const item = (id: string, accelerator?: string): MenuItemOptions => ({
  id,
  text: label(`menu.${id}`),
  accelerator,
  action: () => actions[id]?.(),
});

const separator = (): PredefinedMenuItemOptions => ({ item: "Separator" });
const predefined = (kind: PredefinedMenuItemOptions["item"]): PredefinedMenuItemOptions => ({
  item: kind,
});

/** The bar, as the platform wants it. */
function menuBar(os: string): NonNullable<MenuOptions["items"]> {
  const mac = os === "macos";
  return [
    ...(mac
      ? [
          {
            text: appName,
            items: [
              predefined({ About: { name: appName, version: pkg.version } }),
              separator(),
              item("settings", "CmdOrCtrl+,"),
              separator(),
              predefined("Services"),
              separator(),
              predefined("Hide"),
              predefined("HideOthers"),
              predefined("ShowAll"),
              separator(),
              predefined("Quit"),
            ],
          },
        ]
      : []),
    {
      text: label("menu.file"),
      items: [
        item("rescan", "CmdOrCtrl+Shift+R"),
        // Off macOS there is no app menu to hold 设置 and 退出, so the File menu carries them.
        ...(mac ? [] : [separator(), item("settings", "CmdOrCtrl+,")]),
        separator(),
        mac ? predefined("CloseWindow") : predefined("Quit"),
      ],
    },
    {
      text: label("menu.edit"),
      items: [
        predefined("Undo"),
        predefined("Redo"),
        separator(),
        predefined("Cut"),
        predefined("Copy"),
        predefined("Paste"),
        separator(),
        predefined("SelectAll"),
      ],
    },
    {
      text: label("menu.view"),
      items: [item("theme", "CmdOrCtrl+Alt+T"), separator(), predefined("Fullscreen")],
    },
    {
      text: label("menu.window"),
      items: mac
        ? [predefined("Minimize"), predefined("Maximize"), separator(), predefined("BringAllToFront")]
        : [predefined("Minimize"), predefined("Maximize"), predefined("CloseWindow")],
    },
    { text: label("menu.help"), items: [item("website"), item("checkUpdate")] },
  ];
}

async function refresh(os: string): Promise<void> {
  if (os === "windows") return;
  const menu = await Menu.new({ items: menuBar(os) });
  if (os === "macos") await menu.setAsAppMenu();
  else await menu.setAsWindowMenu();
}

let platform = "";

/**
 * Install the bar once `preflight` says which OS we are on. Failure is only ever worth a log: the
 * menu is an extra way in, and every one of its actions already has a button in the UI.
 */
export function setupMenu(os: string): void {
  platform = os;
  void refresh(os).catch((e) => console.warn("menu:", e));
}

// A language switch must not leave the bar speaking the previous one.
watch(() => messages.locale.value, () => {
  if (platform) void refresh(platform).catch((e) => console.warn("menu:", e));
});
