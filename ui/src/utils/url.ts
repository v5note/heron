// 收藏地址的解析层。后端只存一条 `url`，UI 需要知道它是「能一键安装的仓库」还是「只能打开的链接」，
// 以及仓库的 owner / repo / 子路径 / 落点 slug。「来源与市场」的粘贴条和「我的收藏」的收藏行都要
// 同一个答案，所以这张表放在 utils 里，不留在某个视图内。

export interface Parsed {
  kind: "repo" | "link" | "bad";
  host?: string;
  owner?: string;
  repo?: string;
  branch?: string;
  sub?: string;
  slug?: string;
}

export function parseUrl(raw: string): Parsed {
  const url = raw.trim();
  if (!/^https?:\/\//.test(url)) return { kind: "bad" };
  try {
    const u = new URL(url);
    const host = u.host.toLowerCase().replace(/^www\./, "");
    const repoHost =
      /(^|\.)github\.com$/.test(host) ||
      /(^|\.)gitee\.com$/.test(host) ||
      /(^|\.)modelscope\.cn$/.test(host);
    if (!repoHost) return { kind: "link", host };
    const path = u.pathname.split("/").filter(Boolean);
    const hi = path.findIndex((p) => p === "tree" || p === "blob");
    const base = hi >= 0 ? path.slice(0, hi) : path;
    if (base.length < 2) return { kind: "link", host };
    const branch = hi >= 0 ? path[hi + 1] : "";
    const sub = hi >= 0 ? path.slice(hi + 2).join("/") : "";
    const repo = base[1];
    const slug = (sub.split("/").filter(Boolean).pop() || repo || "").toLowerCase();
    return { kind: "repo", host, owner: base[0], repo, branch, sub, slug };
  } catch {
    return { kind: "bad" };
  }
}

/** 比对用地址归一：去协议/www、`.git`、末尾斜杠与 `/tree/...`，整体小写。 */
export function normUrl(raw: string): string {
  let s = raw.trim().toLowerCase();
  s = s.replace(/^[a-z][a-z0-9+.-]*:\/\//, "").replace(/^www\./, "");
  s = s.replace(/\.git$/, "").replace(/\/+$/, "");
  const hi = s.indexOf("/tree/");
  return hi >= 0 ? s.slice(0, hi) : s;
}
