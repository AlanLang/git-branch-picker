use anyhow::{Context, Result};
use git2::{BranchType, Repository};
use std::path::Path;

pub fn open_repo() -> Result<Repository> {
    Repository::discover(".").context("当前目录不在 git 仓库中，请进入项目目录后重试")
}

pub fn list_remote_branches(repo: &Repository) -> Result<Vec<String>> {
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

pub fn create_and_checkout(repo: &Repository, remote_branch: &str, new_name: &str) -> Result<()> {
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

pub fn create_worktree(
    repo: &Repository,
    remote_branch: &str,
    new_name: &str,
    worktree_path: &Path,
) -> Result<()> {
    let remote_ref = format!("refs/remotes/origin/{}", remote_branch);

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

    {
        let commit = repo.find_commit(commit_oid)?;
        repo.branch(new_name, &commit, false)
            .with_context(|| format!("创建分支 '{}' 失败（分支名可能已存在）", new_name))?;
    }

    let branch_ref = repo.find_reference(&format!("refs/heads/{}", new_name))?;
    {
        let mut opts = git2::WorktreeAddOptions::new();
        opts.reference(Some(&branch_ref));
        repo.worktree(new_name, worktree_path, Some(&opts))
            .context("创建 worktree 失败")?;
    }

    let mut config = repo.config()?;
    config.set_str(&format!("branch.{}.remote", new_name), "origin")?;
    config.set_str(
        &format!("branch.{}.merge", new_name),
        &format!("refs/heads/{}", remote_branch),
    )?;

    Ok(())
}
