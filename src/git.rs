//! 极简 git 封装：通过本机 git 命令行执行，不引入额外依赖。
//! 约定所有函数都是同步阻塞的，调用方放在后台线程里跑。

use std::path::Path;
use std::process::Command;

/// 一个改动文件（暂存区/工作区各有一个状态字符）
#[derive(Clone, Debug)]
pub struct Change {
    /// 相对笔记根目录的路径，既用于 git 命令也用于展示
    pub path: String,
    pub index: char,
    pub worktree: char,
}

impl Change {
    pub fn untracked(&self) -> bool {
        self.index == '?'
    }

    /// 暂存区是否有改动
    pub fn staged(&self) -> bool {
        !matches!(self.index, ' ' | '?')
    }

    /// 暂存区状态字母
    pub fn staged_code(&self) -> char {
        self.index
    }

    /// 工作区状态字母
    pub fn unstaged_code(&self) -> char {
        if self.untracked() {
            'U'
        } else {
            self.worktree
        }
    }
}

#[derive(Clone, Debug, Default)]
pub struct Status {
    pub is_repo: bool,
    pub branch: String,
    pub changes: Vec<Change>,
}

#[derive(Clone, Debug)]
pub struct Commit {
    pub hash: String,
    pub subject: String,
    pub date: String,
}

#[derive(Clone, Debug, Default)]
pub struct Snapshot {
    pub status: Status,
    pub log: Vec<Commit>,
    pub branches: Vec<String>,
}

/// 读取仓库的整体状态（状态 + 最近提交 + 分支），一次性给后台线程用
pub fn snapshot(root: &Path) -> Snapshot {
    let mut snapshot = Snapshot::default();

    if run(root, &["rev-parse", "--show-toplevel"]).is_err() {
        return snapshot; // 不是 git 仓库
    }

    let prefix = run(root, &["rev-parse", "--show-prefix"])
        .map(|s| s.trim().to_string())
        .unwrap_or_default();
    let branch = branch(root);

    snapshot.status = Status {
        is_repo: true,
        branch,
        changes: parse_status(root, &prefix),
    };
    snapshot.log = log(root, 30);
    snapshot.branches = branches(root);
    snapshot
}

/// 当前分支名（空仓库也能拿到默认分支名）
pub fn branch(root: &Path) -> String {
    if let Ok(name) = run(root, &["symbolic-ref", "--short", "-q", "HEAD"]) {
        let name = name.trim();
        if !name.is_empty() {
            return name.to_string();
        }
    }
    run(root, &["rev-parse", "--short", "HEAD"])
        .map(|s| s.trim().to_string())
        .unwrap_or_else(|_| "main".to_string())
}

/// 解析 `git status --porcelain=v1 -z` 输出
/// -z 模式下路径总是相对仓库根目录，这里统一转成相对笔记目录（即 `-C root` 的工作目录）
fn parse_status(root: &Path, prefix: &str) -> Vec<Change> {
    let Ok(bytes) = run_bytes(
        root,
        &[
            "status",
            "--porcelain=v1",
            "-z",
            "--untracked-files=all",
            "--",
            ".",
        ],
    ) else {
        return Vec::new();
    };

    let mut changes = Vec::new();
    let mut fields = bytes.split(|b| *b == 0);
    while let Some(field) = fields.next() {
        if field.len() < 4 {
            continue;
        }
        let index = field[0] as char;
        let worktree = field[1] as char;
        if index == '!' {
            continue; // 被忽略的文件
        }
        let path = String::from_utf8_lossy(&field[3..]).to_string();
        if matches!(index, 'R' | 'C') {
            // -z 模式下重命名的原路径是下一个字段
            let _ = fields.next();
        }
        let path = path.strip_prefix(prefix).unwrap_or(&path).to_string();
        changes.push(Change {
            path,
            index,
            worktree,
        });
    }
    changes
}

/// 取某个改动的补丁文本
pub fn diff(root: &Path, change: &Change, staged: bool) -> Result<String, String> {
    if staged {
        run(root, &["diff", "--cached", "--", &change.path])
    } else if change.untracked() {
        untracked_patch(root, &change.path)
    } else {
        run(root, &["diff", "--", &change.path])
    }
}

/// 为未跟踪文件手工拼一个「全部新增」的补丁
fn untracked_patch(root: &Path, rel: &str) -> Result<String, String> {
    let bytes = std::fs::read(root.join(rel)).map_err(|e| format!("读取文件失败：{e}"))?;
    let text = String::from_utf8_lossy(&bytes).replace("\r\n", "\n");
    let ends_with_newline = text.ends_with('\n');
    let mut lines: Vec<&str> = text.split('\n').collect();
    if ends_with_newline {
        lines.pop();
    }

    let count = lines.len().max(1);
    let mut patch = format!(
        "diff --git a/{rel} b/{rel}\nnew file mode 100644\n--- /dev/null\n+++ b/{rel}\n@@ -0,0 +1,{count} @@\n"
    );
    if lines.is_empty() {
        patch.push_str("+\n");
    } else {
        for line in lines {
            patch.push('+');
            patch.push_str(line);
            patch.push('\n');
        }
    }
    Ok(patch)
}

/// 某次提交的补丁
pub fn show(root: &Path, hash: &str) -> Result<String, String> {
    run(
        root,
        &[
            "show",
            "--format=",
            "--patch",
            "--find-renames",
            hash,
            "--",
            ".",
        ],
    )
}

/// 最近提交（只统计笔记目录下的改动）
pub fn log(root: &Path, limit: usize) -> Vec<Commit> {
    let n = format!("-{limit}");
    let Ok(out) = run(
        root,
        &[
            "log",
            &n,
            "--pretty=format:%h%x1f%ad%x1f%s",
            "--date=format:%m-%d %H:%M",
            "--",
            ".",
        ],
    ) else {
        return Vec::new();
    };

    out.lines()
        .filter_map(|line| {
            let mut parts = line.split('\x1f');
            Some(Commit {
                hash: parts.next()?.to_string(),
                date: parts.next()?.to_string(),
                subject: parts.next().unwrap_or_default().to_string(),
            })
        })
        .collect()
}

/// 本地分支（按最近提交排序）
pub fn branches(root: &Path) -> Vec<String> {
    run(
        root,
        &[
            "for-each-ref",
            "--sort=-committerdate",
            "--format=%(refname:short)",
            "refs/heads",
        ],
    )
    .map(|out| {
        out.lines()
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .map(str::to_string)
            .collect()
    })
    .unwrap_or_default()
}

// ---------- 写操作 ----------

pub fn init(root: &Path) -> Result<String, String> {
    run(root, &["init"])
}

pub fn add(root: &Path, path: &str) -> Result<String, String> {
    run(root, &["add", "--", path])
}

pub fn add_all(root: &Path) -> Result<String, String> {
    run(root, &["add", "-A"])
}

pub fn unstage(root: &Path, path: &str) -> Result<String, String> {
    // 还没有第一个提交时 restore --staged 无法解析 HEAD，退回 rm --cached
    match run(root, &["restore", "--staged", "--", path]) {
        Ok(out) => Ok(out),
        Err(_) => run(root, &["rm", "--cached", "-r", "--quiet", "--", path]),
    }
}

pub fn unstage_all(root: &Path) -> Result<String, String> {
    match run(root, &["reset", "-q"]) {
        Ok(out) => Ok(out),
        Err(_) => run(root, &["rm", "--cached", "-r", "--quiet", "--", "."]),
    }
}

/// 丢弃改动：未跟踪文件直接删除，其余用 restore 还原
pub fn discard(root: &Path, change: &Change) -> Result<String, String> {
    if change.untracked() {
        let target = root.join(&change.path);
        let result = if target.is_dir() {
            std::fs::remove_dir_all(&target)
        } else {
            std::fs::remove_file(&target)
        };
        return result.map(|_| String::new()).map_err(|e| e.to_string());
    }
    run(
        root,
        &["restore", "--staged", "--worktree", "--", &change.path],
    )
}

pub fn commit(root: &Path, message: &str) -> Result<String, String> {
    run(root, &["commit", "-m", message])
}

pub fn checkout(root: &Path, branch: &str) -> Result<String, String> {
    run(root, &["checkout", branch])
}

pub fn push(root: &Path) -> Result<String, String> {
    run(root, &["push"])
}

pub fn pull(root: &Path) -> Result<String, String> {
    run(root, &["pull"])
}

// ---------- 执行 ----------

fn command(root: &Path, args: &[&str]) -> Command {
    let mut cmd = Command::new("git");
    cmd.arg("-C")
        .arg(root)
        .args(["-c", "core.quotepath=false"])
        .args(args)
        .env("GIT_TERMINAL_PROMPT", "0");

    #[cfg(target_os = "windows")]
    {
        use std::os::windows::process::CommandExt;
        cmd.creation_flags(0x0800_0000); // CREATE_NO_WINDOW，避免弹出黑框
    }

    cmd
}

fn run(root: &Path, args: &[&str]) -> Result<String, String> {
    let output = command(root, args)
        .output()
        .map_err(|e| format!("无法执行 git：{e}"))?;

    if output.status.success() {
        Ok(String::from_utf8_lossy(&output.stdout).to_string())
    } else {
        let err = String::from_utf8_lossy(&output.stderr).trim().to_string();
        Err(if err.is_empty() {
            "git 命令执行失败".to_string()
        } else {
            err
        })
    }
}

fn run_bytes(root: &Path, args: &[&str]) -> Result<Vec<u8>, String> {
    let output = command(root, args)
        .output()
        .map_err(|e| format!("无法执行 git：{e}"))?;

    if output.status.success() {
        Ok(output.stdout)
    } else {
        Err(String::from_utf8_lossy(&output.stderr).trim().to_string())
    }
}
