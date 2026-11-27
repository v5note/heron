/**
 * Platform-aware keyboard shortcuts: defaults, formatting, matching, recording, and Tauri menu
 * accelerator conversion.
 *
 * Canonical format uses `+`-joined tokens in fixed modifier order:
 *   macOS:   `Meta+Shift+R`, `Meta+Alt+T`, `Meta+K`, `Meta+,`
 *   Windows: `Ctrl+Shift+R`, `Ctrl+Alt+T`, `Ctrl+K`, `Ctrl+,`
 *
 * Keys are normalized from `KeyboardEvent.code` first so macOS Option chords (where `event.key`
 * produces composed characters like `†` for Option+T) record and match reliably.
 */

export type ShortcutId = "search" | "rescan" | "theme" | "settings";

export const SHORTCUT_IDS: readonly ShortcutId[] = ["search", "rescan", "theme", "settings"];

export type ShortcutMap = Record<ShortcutId, string>;

const MOD_ORDER = ["Ctrl", "Alt", "Shift", "Meta"] as const;
type Mod = (typeof MOD_ORDER)[number];

/**
 * Determine whether to use macOS shortcut conventions.
 * Prefers the backend-reported `preflight.platform` (`"macos" | "windows" | "linux"`),
 * falling back to `navigator` before `preflight` settles (or in browser preview).
 */
export function isMacPlatform(platform?: string): boolean {
  if (platform) return platform === "macos";
  if (typeof navigator === "undefined") return false;
  return /mac|iphone|ipad|ipod/i.test(navigator.platform || navigator.userAgent || "");
}

/** Platform-specific default shortcuts. */
export function defaultShortcuts(mac: boolean): ShortcutMap {
  const primary = mac ? "Meta" : "Ctrl";
  return {
    search: `${primary}+K`,
    rescan: `${primary}+Shift+R`,
    theme: `${primary}+Alt+T`,
    settings: `${primary}+,`,
  };
}

/** Map `KeyboardEvent.code` to a canonical key token. */
const CODE_TO_KEY: Record<string, string> = {
  Comma: ",",
  Period: ".",
  Slash: "/",
  Semicolon: ";",
  Quote: "'",
  BracketLeft: "[",
  BracketRight: "]",
  Backslash: "\\",
  Minus: "-",
  Equal: "=",
  Backquote: "`",
  Space: "Space",
  Enter: "Enter",
  Tab: "Tab",
  ArrowUp: "Up",
  ArrowDown: "Down",
  ArrowLeft: "Left",
  ArrowRight: "Right",
};

const MODIFIER_KEYS = new Set(["Meta", "Control", "Alt", "Shift", "OS", "AltGraph"]);

/** Extract a canonical non-modifier key token from a KeyboardEvent, or null if unsupported. */
export function keyFromEvent(event: KeyboardEvent): string | null {
  if (MODIFIER_KEYS.has(event.key)) return null;

  const { code } = event;
  if (code) {
    if (code.startsWith("Key") && code.length === 4) {
      return code.slice(3).toUpperCase();
    }
    if (code.startsWith("Digit") && code.length === 6) {
      return code.slice(5);
    }
    if (/^F([1-9]|1[0-2])$/.test(code)) {
      return code;
    }
    if (CODE_TO_KEY[code]) {
      return CODE_TO_KEY[code];
    }
  }

  // Fallback for virtual keyboards or events without `code`.
  if (event.key.length === 1) {
    return event.key.toUpperCase();
  }
  if (/^F([1-9]|1[0-2])$/.test(event.key)) {
    return event.key;
  }
  return null;
}

/**
 * Parse a canonical shortcut string into normalized modifiers + key.
 * Returns null if the string is empty or malformed.
 */
export function parseShortcut(combo: string): { mods: Set<Mod>; key: string } | null {
  const trimmed = combo.trim();
  if (!trimmed) return null;
  const parts = trimmed.split("+").filter(Boolean);
  if (!parts.length) return null;

  const mods = new Set<Mod>();
  let key = "";
  for (let i = 0; i < parts.length; i++) {
    const token = parts[i];
    if (i < parts.length - 1) {
      if (token === "Ctrl" || token === "Control") mods.add("Ctrl");
      else if (token === "Alt" || token === "Option") mods.add("Alt");
      else if (token === "Shift") mods.add("Shift");
      else if (token === "Meta" || token === "Cmd" || token === "Command" || token === "Super") mods.add("Meta");
      else return null;
    } else {
      key = token.length === 1 ? token.toUpperCase() : token;
    }
  }
  if (!key) return null;
  return { mods, key };
}

/** Serialize modifiers + key into canonical form. */
function serialize(mods: Set<Mod>, key: string): string {
  const ordered = MOD_ORDER.filter((m) => mods.has(m));
  return [...ordered, key].join("+");
}

/**
 * Capture a shortcut from a KeyboardEvent during recording.
 * Requires at least one command modifier (Ctrl / Alt / Meta) or a function key (F1–F12) so plain
 * typing keys cannot be bound by accident.
 */
export function shortcutFromEvent(event: KeyboardEvent): string | null {
  const key = keyFromEvent(event);
  if (!key) return null;

  const mods = new Set<Mod>();
  if (event.ctrlKey) mods.add("Ctrl");
  if (event.altKey) mods.add("Alt");
  if (event.shiftKey) mods.add("Shift");
  if (event.metaKey) mods.add("Meta");

  const isFunctionKey = /^F([1-9]|1[0-2])$/.test(key);
  const hasCommandMod = mods.has("Ctrl") || mods.has("Alt") || mods.has("Meta");
  if (!hasCommandMod && !isFunctionKey) return null;

  return serialize(mods, key);
}

/** Check whether a KeyboardEvent matches a canonical shortcut string. */
export function matchesShortcut(event: KeyboardEvent, combo: string): boolean {
  const parsed = parseShortcut(combo);
  if (!parsed) return false;

  if (event.ctrlKey !== parsed.mods.has("Ctrl")) return false;
  if (event.altKey !== parsed.mods.has("Alt")) return false;
  if (event.shiftKey !== parsed.mods.has("Shift")) return false;
  if (event.metaKey !== parsed.mods.has("Meta")) return false;

  const key = keyFromEvent(event);
  return key !== null && key === parsed.key;
}

const KEY_LABELS_MAC: Record<string, string> = {
  Space: "Space",
  Enter: "⏎",
  Tab: "⇥",
  Up: "↑",
  Down: "↓",
  Left: "←",
  Right: "→",
};

const KEY_LABELS_WIN: Record<string, string> = {
  Space: "Space",
  Enter: "Enter",
  Tab: "Tab",
  Up: "↑",
  Down: "↓",
  Left: "←",
  Right: "→",
};

/**
 * Format a canonical shortcut for display on the current OS:
 *   macOS:   `⌃⌥⇧⌘K` (Apple symbol order: Ctrl, Alt, Shift, Meta)
 *   Windows: `Ctrl+Alt+Shift+Win+K`
 */
export function formatShortcut(combo: string, mac: boolean): string {
  const parsed = parseShortcut(combo);
  if (!parsed) return "";

  if (mac) {
    const syms: string[] = [];
    if (parsed.mods.has("Ctrl")) syms.push("⌃");
    if (parsed.mods.has("Alt")) syms.push("⌥");
    if (parsed.mods.has("Shift")) syms.push("⇧");
    if (parsed.mods.has("Meta")) syms.push("⌘");
    const k = KEY_LABELS_MAC[parsed.key] ?? parsed.key;
    return `${syms.join("")}${k}`;
  }

  const parts: string[] = [];
  if (parsed.mods.has("Ctrl")) parts.push("Ctrl");
  if (parsed.mods.has("Alt")) parts.push("Alt");
  if (parsed.mods.has("Shift")) parts.push("Shift");
  if (parsed.mods.has("Meta")) parts.push("Win");
  parts.push(KEY_LABELS_WIN[parsed.key] ?? parsed.key);
  return parts.join("+");
}

const TAURI_KEY_MAP: Record<string, string> = {
  Space: "Space",
  Enter: "Return",
  Tab: "Tab",
  Up: "Up",
  Down: "Down",
  Left: "Left",
  Right: "Right",
};

/**
 * Convert a canonical shortcut string to a Tauri menu accelerator string.
 * Returns `undefined` when unbound or invalid so the menu item has no accelerator.
 */
export function toTauriAccelerator(combo: string): string | undefined {
  const parsed = parseShortcut(combo);
  if (!parsed) return undefined;

  const parts: string[] = [];
  if (parsed.mods.has("Meta")) parts.push("Command");
  if (parsed.mods.has("Ctrl")) parts.push("Control");
  if (parsed.mods.has("Alt")) parts.push("Alt");
  if (parsed.mods.has("Shift")) parts.push("Shift");
  parts.push(TAURI_KEY_MAP[parsed.key] ?? parsed.key);
  return parts.join("+");
}
