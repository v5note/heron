-- Adopt 的「就地引用」：正文可以留在 store 之外（软链指向的目录由用户自己维护，例如开发中的仓库）。
-- storage_path 为 NULL 表示正文仍是 store/<slug>，即既有行为。
ALTER TABLE skill ADD COLUMN storage_path TEXT;
