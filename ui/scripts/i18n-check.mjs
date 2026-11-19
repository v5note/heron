// Fails when the zh-CN and en-US locales drift apart (implementation doc §9.2).
import { readFileSync } from "node:fs";

const load = (locale) => JSON.parse(readFileSync(new URL(`../src/i18n/locales/${locale}.json`, import.meta.url), "utf8"));

function flatten(obj, prefix = "", out = new Set()) {
  for (const [key, value] of Object.entries(obj)) {
    const path = prefix ? `${prefix}.${key}` : key;
    if (value && typeof value === "object") flatten(value, path, out);
    else out.add(path);
  }
  return out;
}

const zh = flatten(load("zh-CN"));
const en = flatten(load("en-US"));
const missingInEn = [...zh].filter((k) => !en.has(k));
const missingInZh = [...en].filter((k) => !zh.has(k));

if (missingInEn.length || missingInZh.length) {
  console.error("i18n keys differ between locales");
  if (missingInEn.length) console.error("  missing in en-US:", missingInEn.join(", "));
  if (missingInZh.length) console.error("  missing in zh-CN:", missingInZh.join(", "));
  process.exit(1);
}
console.log(`i18n OK: ${zh.size} keys in both locales`);
