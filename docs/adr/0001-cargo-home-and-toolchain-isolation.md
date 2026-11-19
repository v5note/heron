# ADR-0001: 构建缓存与工具链隔离在工作区内

- 状态：已采纳（2026-11-19）
- 相关：实现文档 §1.4

## 背景

首次 `cargo build` 失败：cargo 无法写入 `$HOME/.cargo/registry/cache`（沙箱/CI 只允许写工作区）。
后续 `rustup target add x86_64-pc-windows-msvc` 同样因 `$HOME/.rustup` 只读失败。

## 决策

1. 仓库内放 `.cargo/config.toml`：`build.target-dir = "target"`。
2. 开发与 CI 统一使用 `CARGO_HOME=<repo>/.cargo-home`、`RUSTUP_HOME=<repo>/.rustup-home`（仅本地；CI 用默认值亦可）。
3. 两者均写入 `.gitignore`。

## 后果

- 任何"从零构建"的环境（沙箱、容器、CI）都能自包含完成；
- 命令需要前缀两个环境变量，已写入 README 与 `just`/npm scripts 计划；
- 不改变产物字节，仅改变缓存位置。
