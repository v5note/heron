-- 本机存在性检测：客户端是否装在这台机器上。
-- 三态而非布尔：NULL = 从未检测过（升级后的老库、首扫前就是这个状态），
-- 0 = 检测过且未命中任何信号，1 = 检测过且命中。
-- 只有 0 让界面置灰；NULL 保持原样，避免升级瞬间满屏灰卡。
-- 这三列刻意不进入 client 的 upsert，见 repos/client.rs::set_presence。
ALTER TABLE client ADD COLUMN detected INTEGER;
ALTER TABLE client ADD COLUMN detected_at TEXT;
ALTER TABLE client ADD COLUMN detect_evidence TEXT NOT NULL DEFAULT '[]';
