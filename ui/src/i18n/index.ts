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

/** Render a backend `Msg` using its stable code (implementation doc §9). */
export function messageFor(code: string, params: Record<string, string> = {}): string {
  const t = i18n.global.t as (key: string, named: Record<string, unknown>) => string;
  const key = `errors.${code}`;
  const text = t(key, params);
  return text === key ? `${code} ${JSON.stringify(params)}` : text;
}
