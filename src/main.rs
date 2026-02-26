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
use std::path::Path;

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
                        if !m.contains(KeyModifiers::CONTROL)
                            && !m.contains(KeyModifiers::ALT) =>
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
    let reference = repo
        .find_reference(&remote_ref)
        .with_context(|| format!("找不到远端分支 'origin/{}'，请先执行 git fetch", remote_branch))?;

    let commit = reference
        .peel_to_commit()
        .context("无法解析提交对象")?;

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
            format!("找不到远端分支 'origin/{}'，请先执行 git fetch", remote_branch)
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
// 入口
// ──────────────────────────────────────────────

fn main() -> Result<()> {
    let args: Vec<String> = std::env::args().collect();
    if args.get(1).map(|s| s.as_str()) == Some("-v")
        || args.get(1).map(|s| s.as_str()) == Some("--version")
    {
        println!("gp {}", env!("CARGO_PKG_VERSION"));
        return Ok(());
    }

    let repo = Repository::discover(".")
        .context("当前目录不在 git 仓库中，请进入项目目录后重试")?;

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
                Err(InquireError::OperationCanceled)
                | Err(InquireError::OperationInterrupted) => false,
                Err(e) => return Err(e.into()),
            };

            if should_cd {
                println!("\n进入 {} ...", worktree_path.display());
                println!("（子 Shell 中，输入 exit 可返回原目录）\n");
                let shell = std::env::var("SHELL").unwrap_or_else(|_| "sh".to_string());
                std::process::Command::new(&shell)
                    .current_dir(&worktree_path)
                    .status()
                    .context("启动 Shell 失败")?;
            }
        }
    }

    Ok(())
}
