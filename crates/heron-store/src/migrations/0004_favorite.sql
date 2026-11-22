-- 「来源与市场 → 我的收藏」：用户收藏的优质开源 Skill（GitHub / Gitee / ModelScope 等），
-- 记录名称 / 地址 / 描述，支持一键安装。地址是安装的来源，收藏本身不复制正文。
CREATE TABLE favorite (
  id          INTEGER PRIMARY KEY AUTOINCREMENT,
  name        TEXT NOT NULL,
  url         TEXT NOT NULL UNIQUE,
  description TEXT NOT NULL DEFAULT '',
  market      TEXT,
  created_at  TEXT NOT NULL
);
CREATE INDEX idx_favorite_created ON favorite(created_at DESC);
