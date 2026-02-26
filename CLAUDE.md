# CLAUDE.md — gp 开发指南

## 项目概述

`gp` 是一个 Rust CLI 工具，帮助开发者从 `origin` 远端分支交互式创建带时间戳的本地工作分支。

二进制名：`gp`，入口：`src/main.rs`，包名：`git-branch-picker`。

## 架构

```
src/
  main.rs       入口：clap 解析 + 调度子命令 + 默认分支选择流程
  cli.rs        clap Derive 定义（Cli struct + Command enum）
  git.rs        Git 操作（open_repo, list_remote_branches, create_and_checkout, create_worktree）
  freq.rs       FrequencyStore（load/save/increment/count）
  worktree.rs   worktree 管理（clean_worktrees, interactive_worktree_list, gather_worktrees, WorktreeEntry）
  ui.rs         UI 交互（Action, WtAction, BranchItem, read_action, read_worktree_action, spawn_shell_in, worktree_is_dirty）
```

## 关键约定

- **分支命名**：`<远端分支名>-<YYYYMMDDHHmmss>`，时间戳由 `chrono::Local::now()` 生成
- **频率存储**：`.git/branch-picker-freq.json`，路径通过 `repo.path()` 获取，仅对当前仓库有效
- **只处理 origin**：目前硬编码只读取 `origin/` 前缀的远端追踪分支
- **错误信息**：统一用中文，通过 `anyhow::context` / `with_context` 附加说明

## 依赖选型原则

- 交互 UI → `inquire`（内置模糊搜索，勿替换为 `dialoguer`）
- Git 操作 → `git2`（原生绑定，勿调用 `git` 子进程）
- 参数解析 → `clap`（derive 模式，子命令定义在 `cli.rs`）
- 避免引入无必要的依赖

## 构建与安装

```bash
cargo build --release          # 产物：target/release/gp
cargo install --path .         # 安装到 ~/.cargo/bin/gp
```

## CI / 自动发布

工作流文件：`.github/workflows/release.yml`，触发条件：push 到 `main`。

流程：
1. 对比 `HEAD` 与 `HEAD~1` 的 `Cargo.toml` `version` 字段
2. 版本号变化时，在 `macos-14`（Apple Silicon）上编译 `aarch64-apple-darwin` 二进制
3. 查找上一版本的 git tag（`v<prev>`），生成两个版本之间的 commit 列表作为 changelog
4. 创建 GitHub Release（tag `v<version>`），附件为 `gp-aarch64-apple-darwin.tar.gz`

**触发新 Release 的方法**：修改 `Cargo.toml` 中的 `version` 字段，然后提交推送即可。

## 常见扩展方向

- **支持多个 remote**：在 `list_remote_branches` 中动态枚举所有 remote，而非硬编码 `origin`
- **自动 fetch**：在列分支前执行 `Remote::fetch()`，需处理认证（SSH / HTTPS）
- **自定义分支名模板**：通过 CLI 参数覆盖默认的时间戳后缀格式
- **添加更多子命令**：在 `cli.rs` 的 `Command` enum 中新增变体即可
