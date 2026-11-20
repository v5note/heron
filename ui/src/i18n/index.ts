import { createI18n } from "vue-i18n";
import en from "./locales/en-US.json";
import zh from "./locales/zh-CN.json";

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
export function messageFor(code: string, params: Record<string, string> = {}): string {
  const t = i18n.global.t as (key: string, named: Record<string, unknown>) => string;
  // Anything that is not an error code (e.g. "adopt.done") is a plain message key.
  if (!code.startsWith("E_")) {
    const key = code;
    const text = t(key, params);
    return text === key ? `${code} ${JSON.stringify(params)}` : text;
  }
  let key = `errors.${code}`;
  const detail = params.detail?.trim();
  if (code === "E_INTERNAL" && detail) key = "errors.E_INTERNAL_DETAIL";
  const text = t(key, params);
  return text === key ? `${code} ${JSON.stringify(params)}` : text;
}
