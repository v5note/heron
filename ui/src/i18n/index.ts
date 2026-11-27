import { createI18n } from "vue-i18n";
import en from "./locales/en-US.json";
import zh from "./locales/zh-CN.json";

export type MessageParams = Record<string, string | number>;
export type Locale = "zh-CN" | "en-US";
export const LOCALES: Locale[] = ["zh-CN", "en-US"];

function detect(): Locale {
  const nav = typeof navigator !== "undefined" ? navigator.language.toLowerCase() : "zh-cn";
  return nav.startsWith("zh") ? "zh-CN" : "en-US";
}

export const i18n = createI18n({
  legacy: false,
  locale: (localStorage.getItem("heron.locale") as Locale) ?? detect(),
  fallbackLocale: "en-US",
  messages: { "zh-CN": zh, "en-US": en },
});

export function setLocale(locale: Locale) {
  i18n.global.locale.value = locale;
  localStorage.setItem("heron.locale", locale);
}

/**
 * Render a backend `Msg` using its stable code (implementation doc §9).
 *
 * `E_INTERNAL` is the catch-all: it also carries transport failures and JavaScript exceptions,
 * where `detail` is the only thing that says what actually broke (e.g. "Command outdated not
 * found"). A bare "Internal error" leaves the user, and us, with nothing to act on, so when a
 * detail is present the more specific variant is rendered instead.
 */
export function messageFor(code: string, params: MessageParams = {}): string {
  // Anything that is not an error code (e.g. "adopt.done") is a plain message key.
  if (!code.startsWith("E_")) return render(code, params);
  return render(errorKeyFor(code, params), params);
}

function errorKeyFor(code: string, params: MessageParams): string {
  const has = (name: string) => String(params[name] ?? "").trim().length > 0;
  if (code === "E_INTERNAL" && has("detail")) return "errors.E_INTERNAL_DETAIL";
  if (code === "E_NOT_MANAGED") {
    if (has("path")) return "errors.E_NOT_MANAGED_OCCUPIED";
    if (has("slug")) return "errors.E_NOT_MANAGED_SLUG";
  }
  if (code === "E_DRIFTED" && has("path")) {
    if (params.reason === "foreign_link") return "errors.E_DRIFTED_FOREIGN_LINK";
    if (params.reason === "copy_modified") return "errors.E_DRIFTED_COPY_MODIFIED";
  }
  return `errors.${code}`;
}

/**
 * A missing key must never page raw JSON at someone: "done {"n":"1"}" is what the placeholder
 * fallback turned into when a param arrived as a number. Say the key and its params in a sentence
 * instead — it still names what broke, for us, while reading like copy for the user.
 */
function render(key: string, params: MessageParams): string {
  const t = i18n.global.t as (key: string, named: MessageParams) => string;
  const text = t(key, params);
  if (text !== key) return text;
  const pairs = Object.entries(params);
  return pairs.length ? `${key} (${pairs.map(([name, value]) => `${name}: ${value}`).join(", ")})` : key;
}
