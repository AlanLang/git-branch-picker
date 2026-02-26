use anyhow::Result;
use git2::{BranchType, Repository};
use inquire::{Confirm, InquireError, Select};
use std::fmt;
use std::fs;
use std::path::PathBuf;

use crate::ui::{read_worktree_action, spawn_shell_in, worktree_is_dirty, WtAction};

pub struct WorktreeEntry {
    pub name: String,
    pub branch: String,
    pub path: PathBuf,
    pub is_main: bool,
}

impl fmt::Display for WorktreeEntry {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{:<30} {}", self.branch, self.path.display())
    }
}

pub fn gather_worktrees(repo: &Repository) -> Result<Vec<WorktreeEntry>> {
    let mut entries = Vec::new();

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

pub fn interactive_worktree_list(repo: &Repository) -> Result<()> {
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

        let action = read_worktree_action(selected.is_main)?;

        match action {
            WtAction::Cd => {
                spawn_shell_in(&selected.path)?;
                return Ok(());
            }
            WtAction::Delete => {
                let wt_name = &selected.name;
                let wt_path = &selected.path;

                let dirty = match Repository::open(wt_path) {
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
                    if let Err(e) = fs::remove_dir_all(wt_path) {
                        eprintln!("✗ 删除目录失败 {}：{}", wt_path.display(), e);
                    } else {
                        match repo.find_worktree(wt_name).and_then(|wt| wt.prune(None)) {
                            Ok(_) => {}
                            Err(e) => {
                                eprintln!("  警告：清理 git 记录失败 {}：{}", wt_name, e)
                            }
                        }
                        println!("✓ 已删除 worktree '{}'", wt_name);
                    }
                }

                entries = gather_worktrees(repo)?;
                if entries.is_empty() {
                    println!("没有剩余的 worktree。");
                    return Ok(());
                }
            }
            WtAction::Back => {
                entries = gather_worktrees(repo)?;
            }
            WtAction::Cancel => {
                return Ok(());
            }
        }
    }
}

pub fn clean_worktrees(repo: &Repository) -> Result<()> {
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
