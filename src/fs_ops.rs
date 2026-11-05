//! 文件系统相关：目录树扫描、名称校验、资源管理器定位。

use std::collections::HashSet;
use std::path::{Path, PathBuf};

use gpui_kit::component::tree::TreeItem;

/// 扫描目录构建树：目录在前、文件在后，各自按名称排序。
/// `expanded` 里记录展开中的目录路径，只影响节点的展开标记。
pub fn build_items(dir: &Path, expanded: &HashSet<String>) -> Vec<TreeItem> {
    let mut dirs: Vec<PathBuf> = Vec::new();
    let mut files: Vec<PathBuf> = Vec::new();

    if let Ok(entries) = std::fs::read_dir(dir) {
        for entry in entries.flatten() {
            let path = entry.path();
            let name = entry.file_name().to_string_lossy().to_string();
            if name.starts_with('.') {
                continue; // 跳过 .git 等隐藏项
            }
            if path.is_dir() {
                dirs.push(path);
            } else {
                files.push(path);
            }
        }
    }

    dirs.sort_by_key(|p| p.file_name().map(|n| n.to_string_lossy().to_lowercase()));
    files.sort_by_key(|p| p.file_name().map(|n| n.to_string_lossy().to_lowercase()));

    let mut items = Vec::with_capacity(dirs.len() + files.len());
    for path in dirs {
        let id = path.to_string_lossy().to_string();
        items.push(
            TreeItem::new(
                id.clone(),
                path.file_name().unwrap_or_default().to_string_lossy(),
            )
            .children(build_items(&path, expanded))
            .expanded(expanded.contains(&id)),
        );
    }
    for path in files {
        items.push(TreeItem::new(
            path.to_string_lossy().to_string(),
            path.file_name().unwrap_or_default().to_string_lossy(),
        ));
    }
    items
}

/// 目录/文件名称是否合法（不能为空、不能含路径分隔符和 Windows 保留字符）
pub fn is_valid_name(name: &str) -> bool {
    !name.is_empty()
        && name != "."
        && name != ".."
        && !name
            .chars()
            .any(|c| matches!(c, '\\' | '/' | ':' | '*' | '?' | '"' | '<' | '>' | '|'))
}

/// 新建文件时的默认名：未命名.md / 未命名 2.md ...
pub fn next_untitled(parent: &Path) -> String {
    let base = "未命名";
    let mut n = 1;
    loop {
        let name = if n == 1 {
            format!("{base}.md")
        } else {
            format!("{base} {n}.md")
        };
        if !parent.join(&name).exists() {
            return name;
        }
        n += 1;
    }
}

/// 新建文件夹的默认名：新建文件夹 / 新建文件夹 2 ...
pub fn next_folder_name(parent: &Path) -> String {
    let base = "新建文件夹";
    let mut n = 1;
    loop {
        let name = if n == 1 {
            base.to_string()
        } else {
            format!("{base} {n}")
        };
        if !parent.join(&name).exists() {
            return name;
        }
        n += 1;
    }
}

/// 欢迎文档：首次运行或切换到空工作区时自动创建并打开
pub const WELCOME_NAME: &str = "欢迎.md";

const WELCOME_TEMPLATE: &str = r#"# 欢迎使用笔记本

这是一个本地 Markdown 笔记本，所有笔记都保存在当前工作区的文件夹里，不联网、不上传。

## 界面

- **左侧资源管理器**：目录树展示工作区内的文件夹与 Markdown 文件
- **左侧源代码管理**：查看 Git 分支、改动与历史提交，暂存、提交、拉取、推送
- **中间编辑区**：多标签，标签上的圆点表示有未保存的修改
- **底部状态栏**：操作提示、字数与行数

## 常用操作

- 点击文件即可打开；右键文件或文件夹可新建笔记、新建文件夹、重命名、删除、在资源管理器中显示
- 侧栏右上角两个图标：切换工作区 / 在资源管理器中打开当前工作区
- 首次打开或切换到空目录时，会自动创建这份欢迎文档

## 三种视图

编辑区右上角三个按钮，或按 `Ctrl+Shift+V` 轮换：

1. **编辑**：所见即所得，直接排版
2. **编辑 + 演示**：左侧源码、右侧演示，中间分隔线可拖动
3. **演示**：只看渲染后的 Markdown

## 排版

顶部工具栏可以随时调整，内容不会丢：

- 字体：黑体 / 宋体 / 霞鹜文楷 / 阿里巴巴普惠体（后两者内置于程序）
- 字号：10 – 36
- 背景：简白 / 雅致 / 黑夜 / 绿茵
- 宽度：宽 / 标准 / 窄

## 快捷键

| 快捷键 | 作用 |
| --- | --- |
| `Ctrl+S` | 保存当前文件 |
| `Ctrl+W` | 关闭当前标签 |
| `Ctrl+Tab` | 切换到下一个标签 |
| `Ctrl+Shift+Tab` | 切换到上一个标签 |
| `Ctrl+Shift+V` | 在编辑 / 编辑 + 演示 / 演示之间切换 |
| `F2` | 重命名选中的文件或文件夹 |
| `Delete` | 删除选中的文件或文件夹 |
| `F5` | 刷新目录树与 Git 状态 |

> 笔记就是普通的 .md 文件，可以直接用其他编辑器打开，也可以用 Git 管理。
"#;

/// 目录是否为空（忽略 .git 等隐藏项）
pub fn is_empty_dir(dir: &Path) -> bool {
    match std::fs::read_dir(dir) {
        Ok(entries) => !entries
            .flatten()
            .any(|entry| !entry.file_name().to_string_lossy().starts_with('.')),
        Err(_) => false,
    }
}

/// 在 `root` 下写入欢迎文档，返回其路径
pub fn write_welcome(root: &Path) -> std::io::Result<PathBuf> {
    let path = root.join(WELCOME_NAME);
    std::fs::write(&path, WELCOME_TEMPLATE)?;
    Ok(path)
}

/// 在系统资源管理器中打开目标所在目录
pub fn reveal_in_explorer(path: &Path) {
    let target = if path.is_dir() {
        path.to_path_buf()
    } else {
        path.parent().unwrap_or(path).to_path_buf()
    };

    #[cfg(target_os = "windows")]
    {
        let _ = std::process::Command::new("explorer").arg(target).spawn();
    }
    #[cfg(target_os = "macos")]
    {
        let _ = std::process::Command::new("open").arg(target).spawn();
    }
    #[cfg(all(not(target_os = "windows"), not(target_os = "macos")))]
    {
        let _ = std::process::Command::new("xdg-open").arg(target).spawn();
    }
}

/// 上次退出时的会话：工作区、打开的文档、当前标签
pub struct Session {
    pub root: Option<PathBuf>,
    pub files: Vec<PathBuf>,
    pub active: Option<usize>,
}

/// 会话文件位置：Windows 用 %APPDATA%\notebook\session.txt
fn session_path() -> Option<PathBuf> {
    let base = std::env::var_os("APPDATA")
        .or_else(|| std::env::var_os("HOME"))
        .map(PathBuf::from)?;
    Some(base.join("notebook").join("session.txt"))
}

/// 读取上次会话；没有记录或读取失败时返回 None
pub fn load_session() -> Option<Session> {
    let text = std::fs::read_to_string(session_path()?).ok()?;
    let mut session = Session {
        root: None,
        files: Vec::new(),
        active: None,
    };

    for line in text.lines() {
        if let Some(value) = line.strip_prefix("root=") {
            if !value.is_empty() {
                session.root = Some(PathBuf::from(value));
            }
        } else if let Some(value) = line.strip_prefix("file=") {
            if !value.is_empty() {
                session.files.push(PathBuf::from(value));
            }
        } else if let Some(value) = line.strip_prefix("active=") {
            session.active = value.parse().ok();
        }
    }
    Some(session)
}

/// 写入当前会话；失败时静默忽略，记住状态不应该影响正常使用
pub fn save_session(root: &Path, files: &[PathBuf], active: Option<usize>) {
    let Some(path) = session_path() else {
        return;
    };
    if let Some(dir) = path.parent() {
        let _ = std::fs::create_dir_all(dir);
    }

    let mut text = format!("root={}\n", root.display());
    for file in files {
        text.push_str(&format!("file={}\n", file.display()));
    }
    if let Some(active) = active {
        text.push_str(&format!("active={active}\n"));
    }
    let _ = std::fs::write(path, text);
}
