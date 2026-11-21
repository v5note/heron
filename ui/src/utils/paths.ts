// 路径的显示层。后端存的是真实绝对路径，只有落到屏幕上才做家目录缩写（Rust 侧同一条规则：
// heron_platform::paths::display）。复制、跳转、发给后端的参数一律用原始路径，绝不用这里的返回值。

/**
 * `home` 前缀换成 `~`：`/Users/me/.claude/skills` → `~/.claude/skills`。
 * 不在家目录下的路径原样返回；后端已经缩写过的（`~/…`）也原样返回，不会二次处理。
 */
export function displayPath(path: string, home: string): string {
  if (!path || !home) return path;
  const shown = path.replace(/\\/g, "/");
  const root = home.replace(/\\/g, "/").replace(/\/+$/, "");
  // 一个字符以内的 home（""、"/"、"."）不配当缩写前缀，否则什么都可能被折成 `~`。
  if (root.length < 2) return path;
  if (shown === root) return "~";
  return shown.startsWith(`${root}/`) ? `~/${shown.slice(root.length + 1)}` : path;
}
