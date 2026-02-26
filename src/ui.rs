use anyhow::{Context, Result};
use crossterm::{
    event::{self, Event, KeyCode, KeyModifiers},
    terminal::{disable_raw_mode, enable_raw_mode},
};
use git2::Repository;
use std::fmt;
use std::io::{self, Write};
use std::path::Path;

pub struct BranchItem {
    pub name: String,
    pub count: u64,
}

impl fmt::Display for BranchItem {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.name)
    }
}

pub enum Action {
    CreateBranch,
    CreateWorktree,
    Cancel,
}

pub fn read_action() -> Result<Action> {
    print!("  [↵] 创建分支  ·  [w / Ctrl+↵] 创建 Worktree  ·  [Esc] 取消：");
    io::stdout().flush()?;

    enable_raw_mode()?;
    let result = (|| -> Result<Action> {
        loop {
            if let Event::Key(key) = event::read()? {
                match (key.code, key.modifiers) {
                    (KeyCode::Enter, m)
                        if !m.contains(KeyModifiers::CONTROL) && !m.contains(KeyModifiers::ALT) =>
                    {
                        return Ok(Action::CreateBranch);
                    }
                    (KeyCode::Enter, m) if m.contains(KeyModifiers::CONTROL) => {
                        return Ok(Action::CreateWorktree);
                    }
                    (KeyCode::Char('w'), _) | (KeyCode::Char('W'), _) => {
                        return Ok(Action::CreateWorktree);
                    }
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

pub enum WtAction {
    Cd,
    Delete,
    Back,
    Cancel,
}

pub fn read_worktree_action(is_main: bool) -> Result<WtAction> {
    if is_main {
        print!("  [↵] 切换  ·  [Esc] 返回：");
    } else {
        print!("  [↵] 切换  ·  [d] 删除  ·  [Esc] 返回：");
    }
    io::stdout().flush()?;

    enable_raw_mode()?;
    let result = (|| -> Result<WtAction> {
        loop {
            if let Event::Key(key) = event::read()? {
                match (key.code, key.modifiers) {
                    (KeyCode::Enter, _) => return Ok(WtAction::Cd),
                    (KeyCode::Char('d'), _) if !is_main => return Ok(WtAction::Delete),
                    (KeyCode::Esc, _) | (KeyCode::Char('q'), _) => return Ok(WtAction::Back),
                    (KeyCode::Char('c'), m) if m.contains(KeyModifiers::CONTROL) => {
                        return Ok(WtAction::Cancel);
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

pub fn worktree_is_dirty(wt_repo: &Repository) -> bool {
    let mut status_opts = git2::StatusOptions::new();
    status_opts
        .include_untracked(true)
        .include_ignored(false)
        .include_unmodified(false);

    match wt_repo.statuses(Some(&mut status_opts)) {
        Ok(s) => !s.is_empty(),
        Err(_) => true,
    }
}

pub fn spawn_shell_in(path: &Path) -> Result<()> {
    println!("\n进入 {} ...", path.display());
    println!("（子 Shell 中，输入 exit 可返回原目录）\n");
    let shell = std::env::var("SHELL").unwrap_or_else(|_| "sh".to_string());
    std::process::Command::new(&shell)
        .current_dir(path)
        .status()
        .context("启动 Shell 失败")?;
    Ok(())
}
