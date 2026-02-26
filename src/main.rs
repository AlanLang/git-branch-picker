use anyhow::{Context, Result};
use chrono::Local;
use crossterm::{
    event::{self, Event, KeyCode, KeyModifiers},
    terminal::{disable_raw_mode, enable_raw_mode},
};
use git2::{BranchType, Repository};
use inquire::{Confirm, InquireError, Select};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::fmt;
use std::fs;
use std::io::{self, Write};
use std::path::{Path, PathBuf};

// ──────────────────────────────────────────────
// 通用辅助函数
// ──────────────────────────────────────────────

fn open_repo() -> Result<Repository> {
    Repository::discover(".").context("当前目录不在 git 仓库中，请进入项目目录后重试")
}

/// 检查 worktree 是否有未提交修改（含 untracked，排除 ignored）
fn worktree_is_dirty(wt_repo: &Repository) -> bool {
    let mut status_opts = git2::StatusOptions::new();
    status_opts
        .include_untracked(true)
        .include_ignored(false)
        .include_unmodified(false);

    match wt_repo.statuses(Some(&mut status_opts)) {
        Ok(s) => !s.is_empty(),
        Err(_) => true, // 无法检查时保守认为 dirty
    }
}

/// 在指定路径下启动子 Shell
fn spawn_shell_in(path: &Path) -> Result<()> {
    println!("\n进入 {} ...", path.display());
    println!("（子 Shell 中，输入 exit 可返回原目录）\n");
    let shell = std::env::var("SHELL").unwrap_or_else(|_| "sh".to_string());
    std::process::Command::new(&shell)
        .current_dir(path)
        .status()
        .context("启动 Shell 失败")?;
    Ok(())
}

// ──────────────────────────────────────────────
// 使用频率持久化
// ──────────────────────────────────────────────

#[derive(Debug, Serialize, Deserialize, Default)]
struct FrequencyStore {
    counts: HashMap<String, u64>,
}

impl FrequencyStore {
    fn load(path: &Path) -> Self {
        fs::read_to_string(path)
            .ok()
            .and_then(|s| serde_json::from_str(&s).ok())
            .unwrap_or_default()
    }

    fn save(&self, path: &Path) -> Result<()> {
        let content = serde_json::to_string_pretty(self)?;
        fs::write(path, content)?;
        Ok(())
    }

    fn increment(&mut self, branch: &str) {
        *self.counts.entry(branch.to_string()).or_insert(0) += 1;
    }

    fn count(&self, branch: &str) -> u64 {
        self.counts.get(branch).copied().unwrap_or(0)
    }
}

// ──────────────────────────────────────────────
// 分支展示（含使用次数）
// ──────────────────────────────────────────────

struct BranchItem {
    name: String,
    count: u64,
}

impl fmt::Display for BranchItem {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.name)
    }
}

// ──────────────────────────────────────────────
// 操作选择（单键读取）
// ──────────────────────────────────────────────

enum Action {
    CreateBranch,
    CreateWorktree,
    Cancel,
}

/// 读取单个按键决定操作：
///   Enter / Space        → 创建本地分支
///   w / W / Ctrl+Enter   → 创建 Worktree
///   Esc / q / Ctrl+C     → 取消
fn read_action() -> Result<Action> {
    print!("  [↵] 创建分支  ·  [w / Ctrl+↵] 创建 Worktree  ·  [Esc] 取消：");
    io::stdout().flush()?;

    enable_raw_mode()?;
    let result = (|| -> Result<Action> {
        loop {
            if let Event::Key(key) = event::read()? {
                match (key.code, key.modifiers) {
                    // Enter（无修饰键或仅 Shift）→ 创建分支
                    (KeyCode::Enter, m)
                        if !m.contains(KeyModifiers::CONTROL) && !m.contains(KeyModifiers::ALT) =>
                    {
                        return Ok(Action::CreateBranch);
                    }
                    // Ctrl+Enter → 创建 Worktree（kitty 协议等现代终端支持）
                    (KeyCode::Enter, m) if m.contains(KeyModifiers::CONTROL) => {
                        return Ok(Action::CreateWorktree);
                    }
                    // w / W → 创建 Worktree（通用回退键）
                    (KeyCode::Char('w'), _) | (KeyCode::Char('W'), _) => {
                        return Ok(Action::CreateWorktree);
                    }
                    // 取消
                    (KeyCode::Esc, _) | (KeyCode::Char('q'), _) => {
                        return Ok(Action::Cancel);
                    }
                    (KeyCode::Char('c'), m) if m.contains(KeyModifiers::CONTROL) => {
                        return Ok(Action::Cancel);
                    }
                    _ => {}
                }
            }
        }
    })();
    let _ = disable_raw_mode();
    println!();
    result
}

// ──────────────────────────────────────────────
// Git 操作
// ──────────────────────────────────────────────

fn list_remote_branches(repo: &Repository) -> Result<Vec<String>> {
    repo.find_remote("origin")
        .context("未找到名为 'origin' 的远程仓库，请先添加 remote：git remote add origin <url>")?;

    let mut branches = Vec::new();
    for item in repo.branches(Some(BranchType::Remote))? {
        let (branch, _) = item?;
        if let Some(name) = branch.name()? {
            if let Some(short) = name.strip_prefix("origin/") {
                if short != "HEAD" {
                    branches.push(short.to_string());
                }
            }
        }
    }
    Ok(branches)
}

fn create_and_checkout(repo: &Repository, remote_branch: &str, new_name: &str) -> Result<()> {
    let remote_ref = format!("refs/remotes/origin/{}", remote_branch);
    let reference = repo.find_reference(&remote_ref).with_context(|| {
        format!(
            "找不到远端分支 'origin/{}'，请先执行 git fetch",
            remote_branch
        )
    })?;

    let commit = reference.peel_to_commit().context("无法解析提交对象")?;

    let branch = repo
        .branch(new_name, &commit, false)
        .with_context(|| format!("创建分支 '{}' 失败（分支名可能已存在）", new_name))?;

    let obj = repo.revparse_single(&format!("refs/heads/{}", new_name))?;
    repo.checkout_tree(&obj, None)
        .context("切换工作区失败，请先提交或暂存当前修改（git stash）")?;
    repo.set_head(branch.get().name().context("分支引用名无效")?)?;

    let mut config = repo.config()?;
    config.set_str(&format!("branch.{}.remote", new_name), "origin")?;
    config.set_str(
        &format!("branch.{}.merge", new_name),
        &format!("refs/heads/{}", remote_branch),
    )?;

    Ok(())
}

/// 在 `worktree_path` 创建一个新 worktree，检出指向 `origin/<remote_branch>` 的新分支 `new_name`。
fn create_worktree(
    repo: &Repository,
    remote_branch: &str,
    new_name: &str,
    worktree_path: &Path,
) -> Result<()> {
    let remote_ref = format!("refs/remotes/origin/{}", remote_branch);

    // 取得远端分支的 commit OID（临时借用立即释放）
    let commit_oid = repo
        .find_reference(&remote_ref)
        .with_context(|| {
            format!(
                "找不到远端分支 'origin/{}'，请先执行 git fetch",
                remote_branch
            )
        })?
        .peel_to_commit()
        .context("无法解析提交对象")?
        .id();

    // 创建本地分支（在独立块内，确保 commit / branch 借用先行释放）
    {
        let commit = repo.find_commit(commit_oid)?;
        repo.branch(new_name, &commit, false)
            .with_context(|| format!("创建分支 '{}' 失败（分支名可能已存在）", new_name))?;
    }

    // 以本地分支为基础创建 worktree
    let branch_ref = repo.find_reference(&format!("refs/heads/{}", new_name))?;
    {
        let mut opts = git2::WorktreeAddOptions::new();
        opts.reference(Some(&branch_ref));
        repo.worktree(new_name, worktree_path, Some(&opts))
            .context("创建 worktree 失败")?;
    }

    // 建立追踪关系
    let mut config = repo.config()?;
    config.set_str(&format!("branch.{}.remote", new_name), "origin")?;
    config.set_str(
        &format!("branch.{}.merge", new_name),
        &format!("refs/heads/{}", remote_branch),
    )?;

    Ok(())
}

// ──────────────────────────────────────────────
// gp clean：清理干净的 worktree
// ──────────────────────────────────────────────

/// 扫描所有 linked worktree，删除满足以下条件的：
///   1. 工作区干净（无未提交修改，包含 untracked 但排除 ignored）
///   2. 所有提交均已推送（HEAD 不领先追踪分支）
fn clean_worktrees(repo: &Repository) -> Result<()> {
    struct WtInfo {
        name: String,
        path: PathBuf,
    }

    let wt_names = repo.worktrees()?;

    if wt_names.is_empty() {
        println!("当前仓库没有任何 worktree。");
        return Ok(());
    }

    println!("正在检查 {} 个 worktree...\n", wt_names.len());

    let mut to_remove: Vec<WtInfo> = Vec::new();
    let mut skipped: Vec<(String, &'static str)> = Vec::new();

    for name_opt in wt_names.iter() {
        let name = match name_opt {
            Some(n) => n,
            None => continue,
        };

        let wt = match repo.find_worktree(name) {
            Ok(w) => w,
            Err(_) => {
                skipped.push((name.to_string(), "无法加载"));
                continue;
            }
        };
        let wt_path = wt.path().to_path_buf();

        // 以独立 Repository 打开 worktree
        let wt_repo = match Repository::open(&wt_path) {
            Ok(r) => r,
            Err(_) => {
                skipped.push((name.to_string(), "无法打开仓库"));
                continue;
            }
        };

        if worktree_is_dirty(&wt_repo) {
            skipped.push((name.to_string(), "有未提交的修改"));
            continue;
        }

        // 获取 HEAD 所在分支
        let head = match wt_repo.head() {
            Ok(h) => h,
            Err(_) => {
                skipped.push((name.to_string(), "无 HEAD"));
                continue;
            }
        };

        if !head.is_branch() {
            skipped.push((name.to_string(), "HEAD 处于游离状态"));
            continue;
        }

        let branch_name = head.shorthand().unwrap_or("unknown").to_string();
        let local_oid = match head.target() {
            Some(oid) => oid,
            None => {
                skipped.push((name.to_string(), "HEAD 无法解析"));
                continue;
            }
        };

        // 获取追踪分支，计算 ahead 数量
        let branch = match wt_repo.find_branch(&branch_name, BranchType::Local) {
            Ok(b) => b,
            Err(_) => {
                skipped.push((name.to_string(), "找不到本地分支"));
                continue;
            }
        };

        let upstream = match branch.upstream() {
            Ok(u) => u,
            Err(_) => {
                skipped.push((name.to_string(), "无追踪分支"));
                continue;
            }
        };

        let upstream_oid = match upstream.get().target() {
            Some(oid) => oid,
            None => {
                skipped.push((name.to_string(), "追踪分支无法解析"));
                continue;
            }
        };

        let (ahead, _behind) = match wt_repo.graph_ahead_behind(local_oid, upstream_oid) {
            Ok(r) => r,
            Err(_) => {
                skipped.push((name.to_string(), "无法比较分支进度"));
                continue;
            }
        };

        if ahead > 0 {
            skipped.push((name.to_string(), "有未推送的提交"));
            continue;
        }

        to_remove.push(WtInfo {
            name: name.to_string(),
            path: wt_path,
        });
    }

    // 展示跳过的条目
    if !skipped.is_empty() {
        println!("跳过（有改动或未推送提交）：");
        for (name, reason) in &skipped {
            println!("  ✗  {:<40} {}", name, reason);
        }
        println!();
    }

    if to_remove.is_empty() {
        println!("没有可清理的 worktree。");
        return Ok(());
    }

    println!("可安全清理的 worktree：");
    for info in &to_remove {
        println!("  •  {:<40} {}", info.name, info.path.display());
    }
    println!();

    let confirm = match Confirm::new(&format!("确认删除以上 {} 个 worktree？", to_remove.len()))
        .with_default(false)
        .prompt()
    {
        Ok(v) => v,
        Err(InquireError::OperationCanceled) | Err(InquireError::OperationInterrupted) => false,
        Err(e) => return Err(e.into()),
    };

    if !confirm {
        println!("已取消。");
        return Ok(());
    }

    let mut removed = 0;
    for info in &to_remove {
        // 先删目录，再清 git 内部记录（目录消失后 worktree 变为 invalid，prune(None) 即可处理）
        if let Err(e) = fs::remove_dir_all(&info.path) {
            eprintln!("✗ 删除目录失败 {}：{}", info.path.display(), e);
            continue;
        }
        match repo.find_worktree(&info.name).and_then(|wt| wt.prune(None)) {
            Ok(_) => {}
            Err(e) => eprintln!("  警告：清理 git 记录失败 {}：{}", info.name, e),
        }
        println!("✓ {}  ({})", info.name, info.path.display());
        removed += 1;
    }

    println!("\n已清理 {} 个 worktree。", removed);
    Ok(())
}

// ──────────────────────────────────────────────
// gp w：交互式 worktree 列表
// ──────────────────────────────────────────────

struct WorktreeEntry {
    name: String,
    branch: String,
    path: PathBuf,
    is_main: bool,
}

impl fmt::Display for WorktreeEntry {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{:<30} {}", self.branch, self.path.display())
    }
}

fn gather_worktrees(repo: &Repository) -> Result<Vec<WorktreeEntry>> {
    let mut entries = Vec::new();

    // 主 worktree
    if let Some(workdir) = repo.workdir() {
        let branch = repo
            .head()
            .ok()
            .and_then(|h| h.shorthand().map(|s| s.to_string()))
            .unwrap_or_else(|| "(detached)".to_string());
        entries.push(WorktreeEntry {
            name: "(main)".to_string(),
            branch,
            path: workdir.to_path_buf(),
            is_main: true,
        });
    }

    // linked worktrees
    let wt_names = repo.worktrees()?;
    for name_opt in wt_names.iter() {
        let name = match name_opt {
            Some(n) => n,
            None => continue,
        };
        let wt = match repo.find_worktree(name) {
            Ok(w) => w,
            Err(_) => continue,
        };
        let wt_path = wt.path().to_path_buf();
        let branch = match Repository::open(&wt_path) {
            Ok(r) => r
                .head()
                .ok()
                .and_then(|h| h.shorthand().map(|s| s.to_string()))
                .unwrap_or_else(|| "(detached)".to_string()),
            Err(_) => "(unknown)".to_string(),
        };
        entries.push(WorktreeEntry {
            name: name.to_string(),
            branch,
            path: wt_path,
            is_main: false,
        });
    }

    Ok(entries)
}

fn interactive_worktree_list(repo: &Repository) -> Result<()> {
    let mut entries = gather_worktrees(repo)?;

    if entries.is_empty() {
        println!("当前仓库没有任何 worktree。");
        return Ok(());
    }

    loop {
        let selected = match Select::new("选择 worktree：", entries)
            .with_help_message("↑↓ 移动 · Enter 选择 · Esc 退出")
            .prompt()
        {
            Ok(item) => item,
            Err(InquireError::OperationCanceled) | Err(InquireError::OperationInterrupted) => {
                return Ok(());
            }
            Err(e) => return Err(e.into()),
        };

        let path = selected.path.clone();

        // 主 worktree 只能切换，不能删除
        if selected.is_main {
            spawn_shell_in(&path)?;
            return Ok(());
        }

        // linked worktree：选择操作
        let actions = vec!["切换（cd）", "删除", "返回列表"];
        let action = match Select::new(
            &format!("对 '{}' 执行什么操作？", selected.branch),
            actions,
        )
        .prompt()
        {
            Ok(a) => a,
            Err(InquireError::OperationCanceled) | Err(InquireError::OperationInterrupted) => {
                return Ok(());
            }
            Err(e) => return Err(e.into()),
        };

        match action {
            "切换（cd）" => {
                spawn_shell_in(&path)?;
                return Ok(());
            }
            "删除" => {
                let wt_name = selected.name.clone();
                let wt_path = selected.path.clone();

                let dirty = match Repository::open(&wt_path) {
                    Ok(r) => worktree_is_dirty(&r),
                    Err(_) => true,
                };

                let prompt = if dirty {
                    format!("⚠ worktree '{}' 有未提交修改，确认删除？", wt_name)
                } else {
                    format!("确认删除 worktree '{}'？", wt_name)
                };

                let confirm = match Confirm::new(&prompt).with_default(false).prompt() {
                    Ok(v) => v,
                    Err(InquireError::OperationCanceled)
                    | Err(InquireError::OperationInterrupted) => false,
                    Err(e) => return Err(e.into()),
                };

                if confirm {
                    if let Err(e) = fs::remove_dir_all(&wt_path) {
                        eprintln!("✗ 删除目录失败 {}：{}", wt_path.display(), e);
                    } else {
                        match repo.find_worktree(&wt_name).and_then(|wt| wt.prune(None)) {
                            Ok(_) => {}
                            Err(e) => {
                                eprintln!("  警告：清理 git 记录失败 {}：{}", wt_name, e)
                            }
                        }
                        println!("✓ 已删除 worktree '{}'", wt_name);
                    }
                }

                // 刷新列表继续循环
                entries = gather_worktrees(repo)?;
                if entries.is_empty() {
                    println!("没有剩余的 worktree。");
                    return Ok(());
                }
            }
            // "返回列表" 或其他
            _ => {
                // 刷新列表继续循环
                entries = gather_worktrees(repo)?;
            }
        }
    }
}

// ──────────────────────────────────────────────
// 帮助信息
// ──────────────────────────────────────────────

fn print_help() {
    println!(
        "gp — 交互式 Git 分支创建工具

用法：
  gp              选择远端分支，创建本地工作分支或 worktree
  gp w            列出所有 worktree，支持切换和删除
  gp clean        清理干净的 worktree（无修改、无未推送提交）
  gp -v           显示版本号
  gp -h, --help   显示此帮助信息"
    );
}

// ──────────────────────────────────────────────
// 入口
// ──────────────────────────────────────────────

fn main() -> Result<()> {
    let args: Vec<String> = std::env::args().collect();
    match args.get(1).map(|s| s.as_str()) {
        Some("-v") | Some("--version") => {
            println!("gp {}", env!("CARGO_PKG_VERSION"));
            return Ok(());
        }
        Some("-h") | Some("--help") => {
            print_help();
            return Ok(());
        }
        Some("clean") => {
            let repo = open_repo()?;
            return clean_worktrees(&repo);
        }
        Some("w") => {
            let repo = open_repo()?;
            return interactive_worktree_list(&repo);
        }
        _ => {}
    }

    let repo = open_repo()?;

    let freq_path = repo.path().join("branch-picker-freq.json");
    let mut freq = FrequencyStore::load(&freq_path);

    let branch_names = list_remote_branches(&repo)?;

    if branch_names.is_empty() {
        eprintln!("origin 下没有找到任何远端分支。");
        eprintln!("提示：先执行 `git fetch` 拉取最新分支信息。");
        return Ok(());
    }

    let mut items: Vec<BranchItem> = branch_names
        .into_iter()
        .map(|name| {
            let count = freq.count(&name);
            BranchItem { name, count }
        })
        .collect();

    items.sort_by(|a, b| b.count.cmp(&a.count).then_with(|| a.name.cmp(&b.name)));

    println!("找到 {} 个远端分支（按使用频率排序）\n", items.len());

    let selected = match Select::new("选择要基于的远端分支：", items)
        .with_help_message("输入关键字过滤  ·  ↑↓ 移动  ·  Enter 确认  ·  Esc 取消")
        .prompt()
    {
        Ok(item) => item,
        Err(InquireError::OperationCanceled) | Err(InquireError::OperationInterrupted) => {
            println!("已取消。");
            return Ok(());
        }
        Err(e) => return Err(e.into()),
    };

    let branch_name = selected.name.clone();

    // 读取单键操作
    let action = read_action()?;

    match action {
        Action::Cancel => {
            println!("已取消。");
            return Ok(());
        }

        Action::CreateBranch => {
            freq.increment(&branch_name);
            freq.save(&freq_path)?;

            let timestamp = Local::now().format("%Y%m%d%H%M%S");
            let new_branch = format!("{}-{}", branch_name, timestamp);

            println!("\n正在创建分支 '{}' ...", new_branch);
            create_and_checkout(&repo, &branch_name, &new_branch)?;

            println!("\n✓ 已切换到新分支：{}", new_branch);
            println!("  追踪自：origin/{}", branch_name);
        }

        Action::CreateWorktree => {
            freq.increment(&branch_name);
            freq.save(&freq_path)?;

            let timestamp = Local::now().format("%Y%m%d%H%M%S");
            let new_branch = format!("{}-{}", branch_name, timestamp);

            // worktree 放在 repo 根目录的同级：../new_branch
            let repo_root = repo
                .workdir()
                .context("裸仓库不支持创建 worktree")?
                .to_path_buf();
            let parent_dir = repo_root.parent().context("无法获取仓库父目录")?;
            let worktree_path = parent_dir.join(&new_branch);

            println!("\n正在创建 Worktree '{}'...", new_branch);
            println!("  路径：{}", worktree_path.display());

            create_worktree(&repo, &branch_name, &new_branch, &worktree_path)?;

            println!("\n✓ Worktree 已创建");
            println!("  分支：{}  追踪自：origin/{}", new_branch, branch_name);
            println!("  路径：{}", worktree_path.display());

            let should_cd = match Confirm::new("是否切换到 worktree 目录？")
                .with_default(true)
                .prompt()
            {
                Ok(v) => v,
                Err(InquireError::OperationCanceled) | Err(InquireError::OperationInterrupted) => {
                    false
                }
                Err(e) => return Err(e.into()),
            };

            if should_cd {
                spawn_shell_in(&worktree_path)?;
            }
        }
    }

    Ok(())
}
