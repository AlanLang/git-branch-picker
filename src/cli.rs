use clap::{Parser, Subcommand};

#[derive(Parser)]
#[command(name = "gp", version, about = "交互式 Git 分支创建工具")]
pub struct Cli {
    #[command(subcommand)]
    pub command: Option<Command>,
}

#[derive(Subcommand)]
pub enum Command {
    /// 列出所有 worktree，支持切换和删除
    W,
    /// 清理干净的 worktree（无修改、无未推送提交）
    Clean,
}
