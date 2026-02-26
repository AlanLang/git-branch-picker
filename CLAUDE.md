# CLAUDE.md — gp 开发指南

## 项目概述

`gp` 是一个 Rust CLI 工具，帮助开发者从 `origin` 远端分支交互式创建带时间戳的本地工作分支。

二进制名：`gp`，入口：`src/main.rs`，包名：`git-branch-picker`。

## 架构

```
src/main.rs          唯一源文件，所有逻辑在此
  FrequencyStore     读写 .git/branch-picker-freq.json，记录各分支被选次数
  BranchItem         包装分支名和频率，实现 Display 供 inquire 展示
  list_remote_branches()    枚举 origin/* 远端追踪分支
  create_and_checkout()     创建本地分支 + checkout + 设置追踪关系
  main()             参数解析 → 列分支 → 排序 → 交互选择 → 创建分支
```

## 关键约定

- **分支命名**：`<远端分支名>-<YYYYMMDDHHmmss>`，时间戳由 `chrono::Local::now()` 生成
- **频率存储**：`.git/branch-picker-freq.json`，路径通过 `repo.path()` 获取，仅对当前仓库有效
- **只处理 origin**：目前硬编码只读取 `origin/` 前缀的远端追踪分支
- **错误信息**：统一用中文，通过 `anyhow::context` / `with_context` 附加说明

## 依赖选型原则

- 交互 UI → `inquire`（内置模糊搜索，勿替换为 `dialoguer`）
- Git 操作 → `git2`（原生绑定，勿调用 `git` 子进程）
- 参数解析 → 当前手动解析 `std::env::args()`，如需扩展子命令再引入 `clap`
- 避免引入无必要的依赖

## 构建与安装

```bash
cargo build --release          # 产物：target/release/gp
cargo install --path .         # 安装到 ~/.cargo/bin/gp
```

## 常见扩展方向

- **支持多个 remote**：在 `list_remote_branches` 中动态枚举所有 remote，而非硬编码 `origin`
- **自动 fetch**：在列分支前执行 `Remote::fetch()`，需处理认证（SSH / HTTPS）
- **自定义分支名模板**：通过 CLI 参数覆盖默认的时间戳后缀格式
- **引入 clap**：当参数超过 2 个时替换当前手动解析逻辑
