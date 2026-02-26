# gp — Git Branch Picker

从 `origin` 远端分支交互式创建本地工作分支，自动追加时间戳，并记录使用频率排序。

## 功能

- 列出 `origin` 的所有远端分支，按**使用频率**降序排列
- 实时**模糊过滤**，输入关键字即可缩小范围
- 选中后自动创建本地分支，名称格式：`<远端分支名>-<YYYYMMDDHHmmss>`
- 自动切换到新分支，并建立与 `origin` 的**追踪关系**
- 使用频率持久化在 `.git/branch-picker-freq.json`，仅对当前仓库生效

## 安装

```bash
git clone <this-repo>
cd start-code
cargo install --path .
```

需要 Rust 工具链（推荐通过 [rustup](https://rustup.rs) 安装）。

## 使用

在任意 git 项目目录中执行：

```bash
gp
```

交互操作：

| 按键 | 动作 |
|------|------|
| 输入字符 | 模糊过滤分支 |
| `↑` / `↓` | 移动光标 |
| `Enter` | 确认选择并创建分支 |
| `Esc` / `Ctrl+C` | 取消退出 |

查看版本：

```bash
gp -v
gp --version
```

## 示例

```
找到 8 个远端分支（按使用频率排序）

? 选择要基于的远端分支：
> develop
  main
  feature/auth
  feature/payments

正在创建分支 'develop-20260226143052' ...

✓ 已切换到新分支：develop-20260226143052
  追踪自：origin/develop
```

## 依赖

| 库 | 用途 |
|----|------|
| `git2` | Git 操作（读取分支、创建分支、切换、追踪配置） |
| `inquire` | 交互式 TUI 选择，内置模糊搜索 |
| `serde` + `serde_json` | 频率数据序列化 |
| `chrono` | 时间戳生成 |
| `anyhow` | 错误处理与友好提示 |
