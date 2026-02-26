# gp — Git Branch Picker

从 `origin` 远端分支交互式创建本地工作分支，自动追加时间戳，并记录使用频率排序。

## 功能

- 列出 `origin` 的所有远端分支，按**使用频率**降序排列
- 实时**模糊过滤**，输入关键字即可缩小范围
- 选中后可选择**创建本地分支**（切换到新分支）或**创建 Worktree**（在同级目录）
- 自动建立与 `origin` 的**追踪关系**
- 使用频率持久化在 `.git/branch-picker-freq.json`，仅对当前仓库生效
- `gp clean` 一键清理所有工作区干净且已全部推送的 worktree

## 安装

```bash
git clone <this-repo>
cd start-code
cargo install --path .
```

需要 Rust 工具链（推荐通过 [rustup](https://rustup.rs) 安装）。

## 使用

### 创建分支 / Worktree

在任意 git 项目目录中执行：

```bash
gp
```

用 `↑↓` 移动、输入关键字过滤，选中分支后通过按键决定操作：

| 按键 | 动作 |
|------|------|
| 输入字符 | 模糊过滤分支 |
| `↑` / `↓` | 移动光标 |
| `Enter` | 确认选择 |
| `Esc` / `Ctrl+C` | 取消退出 |

确认选择后，再按一键决定操作模式：

| 按键 | 动作 |
|------|------|
| `Enter` | 创建本地分支并切换 |
| `w`（或支持 kitty 协议终端上的 `Ctrl+Enter`） | 在仓库同级目录创建 Worktree |
| `Esc` / `q` | 取消 |

创建 Worktree 后会询问是否切换到该目录（默认 Y），选择后会在 Worktree 路径下打开子 Shell，`exit` 即可返回原目录。

### 清理 Worktree

```bash
gp clean
```

扫描所有 linked worktree，仅删除同时满足以下两个条件的：

1. **工作区干净**：无未提交修改（含 untracked 文件，排除 `.gitignore` 的文件）
2. **所有提交已推送**：HEAD 不领先追踪分支

列出可清理和跳过的条目后，需手动确认（默认 N）才会执行删除。

### 查看版本

```bash
gp -V
gp --version
```

## 示例

### 创建本地分支

```
找到 8 个远端分支（按使用频率排序）

? 选择要基于的远端分支：develop
  [↵] 创建分支  ·  [w / Ctrl+↵] 创建 Worktree  ·  [Esc] 取消：

正在创建分支 'develop-20260226143052' ...

✓ 已切换到新分支：develop-20260226143052
  追踪自：origin/develop
```

### 创建 Worktree

```
? 选择要基于的远端分支：main
  [↵] 创建分支  ·  [w / Ctrl+↵] 创建 Worktree  ·  [Esc] 取消：w

正在创建 Worktree 'main-20260226153000'...
  路径：/Users/alan/code/main-20260226153000

✓ Worktree 已创建
  分支：main-20260226153000  追踪自：origin/main
  路径：/Users/alan/code/main-20260226153000

? 是否切换到 worktree 目录？ (Y/n)
```

### 清理 Worktree

```
$ gp clean
正在检查 3 个 worktree...

跳过（有改动或未推送提交）：
  ✗  feature-login-20260201120000           有未提交的修改
  ✗  hotfix-crash-20260210093000            有未推送的提交

可安全清理的 worktree：
  •  main-20260115080000                    /Users/alan/code/main-20260115080000

? 确认删除以上 1 个 worktree？ (y/N) y
✓ main-20260115080000  (/Users/alan/code/main-20260115080000)

已清理 1 个 worktree。
```

## 依赖

| 库 | 用途 |
|----|------|
| `git2` | Git 操作（读取分支、创建分支、worktree、追踪配置） |
| `clap` | CLI 参数解析与子命令管理（derive 模式） |
| `inquire` | 交互式 TUI 选择，内置模糊搜索 |
| `crossterm` | 单键操作模式读取 |
| `serde` + `serde_json` | 频率数据序列化 |
| `chrono` | 时间戳生成 |
| `anyhow` | 错误处理与友好提示 |
