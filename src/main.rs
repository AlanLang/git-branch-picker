mod cli;
mod freq;
mod git;
mod ui;
mod worktree;

use anyhow::{Context, Result};
use chrono::Local;
use clap::Parser;
use inquire::{Confirm, InquireError, Select, Text};

use cli::{Cli, Command};
use freq::FrequencyStore;
use git::{create_and_checkout, create_worktree, list_remote_branches, open_repo};
use ui::{read_action, spawn_shell_in, Action, BranchItem};
use worktree::{clean_worktrees, interactive_worktree_list};

fn main() -> Result<()> {
    let cli = Cli::parse();

    match cli.command {
        Some(Command::W) => {
            let repo = open_repo()?;
            return interactive_worktree_list(&repo);
        }
        Some(Command::Clean) => {
            let repo = open_repo()?;
            return clean_worktrees(&repo);
        }
        None => {}
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
            let default_name = format!("{}-{}", branch_name, timestamp);

            let new_branch = match Text::new("Worktree 名称：")
                .with_initial_value(&default_name)
                .prompt()
            {
                Ok(name) => {
                    let name = name.trim().to_string();
                    if name.is_empty() {
                        default_name
                    } else {
                        name
                    }
                }
                Err(InquireError::OperationCanceled) | Err(InquireError::OperationInterrupted) => {
                    println!("已取消。");
                    return Ok(());
                }
                Err(e) => return Err(e.into()),
            };

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
