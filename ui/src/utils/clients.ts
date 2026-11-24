// 客户端身份的视觉映射：线描图标走 AppIcon 那一套 path（与 UI 其余图标同笔画），品牌色只用来
// 「认出是哪家」。发现页的筛选菜单和概览的落点卡片共用这一张表，两处不会各自漂移。
// 色值取中间亮度，同一个 hex 在深浅两个表面上都读得出。
import type { IconName } from "../components/AppIcon.vue";

export const CLIENT_ICONS: Record<string, IconName> = {
  agents: "globe",
  claude: "spark",
  codex: "terminal",
  cursor: "pointer",
  opencode: "braces",
  workbuddy: "robot",
  antigravity: "rocket",
  trae: "star",
  // Trae CN 是同一家产品的国内构建，沿用星形表示同源，靠颜色区分两个落点。
  "trae-cn": "star",
  qoder: "cube",
  grok: "bolt",
  cline: "code",
  copilot: "sparkles",
  heron: "home",
};

export const CLIENT_COLORS: Record<string, string> = {
  agents: "#339af0",
  claude: "#e8590c",
  codex: "#0ca678",
  cursor: "#9c36b5",
  opencode: "#f08c00",
  workbuddy: "#e64980",
  antigravity: "#1098ad",
  trae: "#4263eb",
  "trae-cn": "#1971c2",
  qoder: "#d6336c",
  grok: "#e03131",
  cline: "#2f9e44",
  copilot: "#7048e8",
  heron: "#6c757d",
};

/** 「全部」是筛选态而不是客户端，借九宫格表示；未知 id 一律落到「来源」图标。 */
export function clientIcon(id: string): IconName {
  if (id === "all") return "grid";
  return CLIENT_ICONS[id] ?? "sources";
}

export function clientColor(id: string): string {
  return CLIENT_COLORS[id] ?? "#868e96";
}
