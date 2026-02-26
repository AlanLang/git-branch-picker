use anyhow::{Context, Result};
use chrono::Local;
use git2::{BranchType, Repository};
use inquire::{InquireError, Select};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::fmt;
use std::fs;
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
// Git 操作
// ──────────────────────────────────────────────

fn list_remote_branches(repo: &Repository) -> Result<Vec<String>> {
    // 检查 origin 远程是否存在
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
    // 找到远端追踪引用
    let remote_ref = format!("refs/remotes/origin/{}", remote_branch);
    let reference = repo
        .find_reference(&remote_ref)
        .with_context(|| format!("找不到远端分支 'origin/{}'，请先执行 git fetch", remote_branch))?;

    let commit = reference
        .peel_to_commit()
        .context("无法解析提交对象")?;

    // 创建本地分支
    let branch = repo
        .branch(new_name, &commit, false)
        .with_context(|| format!("创建分支 '{}' 失败（分支名可能已存在）", new_name))?;

    // 切换到新分支
    let obj = repo.revparse_single(&format!("refs/heads/{}", new_name))?;
    repo.checkout_tree(&obj, None)
        .context("切换工作区失败，请先提交或暂存当前修改（git stash）")?;
    repo.set_head(branch.get().name().context("分支引用名无效")?)?;

    // 建立追踪关系，使 git push/pull 可以直接使用
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
    // 处理 -v / --version 参数
    let args: Vec<String> = std::env::args().collect();
    if args.get(1).map(|s| s.as_str()) == Some("-v")
        || args.get(1).map(|s| s.as_str()) == Some("--version")
    {
        println!("gp {}", env!("CARGO_PKG_VERSION"));
        return Ok(());
    }

    // 向上查找 git 仓库
    let repo = Repository::discover(".")
        .context("当前目录不在 git 仓库中，请进入项目目录后重试")?;

    // 频率数据存放在 .git/branch-picker-freq.json
    let freq_path = repo.path().join("branch-picker-freq.json");
    let mut freq = FrequencyStore::load(&freq_path);

    // 获取远端分支列表
    let branch_names = list_remote_branches(&repo)?;

    if branch_names.is_empty() {
        eprintln!("origin 下没有找到任何远端分支。");
        eprintln!("提示：先执行 `git fetch` 拉取最新分支信息。");
        return Ok(());
    }

    // 按使用频率降序排列，相同频率按名称字母升序
    let mut items: Vec<BranchItem> = branch_names
        .into_iter()
        .map(|name| {
            let count = freq.count(&name);
            BranchItem { name, count }
        })
        .collect();

    items.sort_by(|a, b| b.count.cmp(&a.count).then_with(|| a.name.cmp(&b.name)));

    // 展示总数提示
    println!("找到 {} 个远端分支（按使用频率排序）\n", items.len());

    // 交互式选择（支持实时模糊过滤）
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

    // 记录使用频率
    freq.increment(&branch_name);
    freq.save(&freq_path)?;

    // 生成新分支名：<远端分支名>-<年月日时分秒>
    let timestamp = Local::now().format("%Y%m%d%H%M%S");
    let new_branch = format!("{}-{}", branch_name, timestamp);

    println!("\n正在创建分支 '{}' ...", new_branch);

    create_and_checkout(&repo, &branch_name, &new_branch)?;

    println!("\n✓ 已切换到新分支：{}", new_branch);
    println!("  追踪自：origin/{}", branch_name);

    Ok(())
}
