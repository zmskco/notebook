//! 文件 / 文件夹图标：复用 VS Code「Material Icon Theme」的彩色 SVG。
//!
//! 内置的 lucide 图标只能按文字颜色整体着色，无法还原材质图标的多种颜色，
//! 所以启动时先用 GPUI 的 SVG 渲染器把图标栅格化为图片，树上和标签上直接画位图。

use std::collections::HashMap;
use std::path::Path;
use std::sync::Arc;

use gpui_kit::assets::IconName;
use gpui_kit::component::Icon;
use gpui_kit::*;

const ICON_FILES: &[(&str, &[u8])] = &[
    ("file", include_bytes!("../assets/file-icons/file.svg")),
    (
        "document",
        include_bytes!("../assets/file-icons/document.svg"),
    ),
    (
        "markdown",
        include_bytes!("../assets/file-icons/markdown.svg"),
    ),
    ("image", include_bytes!("../assets/file-icons/image.svg")),
    ("json", include_bytes!("../assets/file-icons/json.svg")),
    ("toml", include_bytes!("../assets/file-icons/toml.svg")),
    ("yaml", include_bytes!("../assets/file-icons/yaml.svg")),
    ("lock", include_bytes!("../assets/file-icons/lock.svg")),
    ("rust", include_bytes!("../assets/file-icons/rust.svg")),
    ("python", include_bytes!("../assets/file-icons/python.svg")),
    (
        "javascript",
        include_bytes!("../assets/file-icons/javascript.svg"),
    ),
    (
        "typescript",
        include_bytes!("../assets/file-icons/typescript.svg"),
    ),
    ("react", include_bytes!("../assets/file-icons/react.svg")),
    (
        "react_ts",
        include_bytes!("../assets/file-icons/react_ts.svg"),
    ),
    ("go", include_bytes!("../assets/file-icons/go.svg")),
    ("java", include_bytes!("../assets/file-icons/java.svg")),
    ("c", include_bytes!("../assets/file-icons/c.svg")),
    ("cpp", include_bytes!("../assets/file-icons/cpp.svg")),
    ("h", include_bytes!("../assets/file-icons/h.svg")),
    ("console", include_bytes!("../assets/file-icons/console.svg")),
    ("table", include_bytes!("../assets/file-icons/table.svg")),
    ("diff", include_bytes!("../assets/file-icons/diff.svg")),
    ("git", include_bytes!("../assets/file-icons/git.svg")),
    ("settings", include_bytes!("../assets/file-icons/settings.svg")),
    ("pdf", include_bytes!("../assets/file-icons/pdf.svg")),
    ("word", include_bytes!("../assets/file-icons/word.svg")),
    ("zip", include_bytes!("../assets/file-icons/zip.svg")),
    ("audio", include_bytes!("../assets/file-icons/audio.svg")),
    ("video", include_bytes!("../assets/file-icons/video.svg")),
    ("font", include_bytes!("../assets/file-icons/font.svg")),
    (
        "database",
        include_bytes!("../assets/file-icons/database.svg"),
    ),
    ("license", include_bytes!("../assets/file-icons/license.svg")),
    ("readme", include_bytes!("../assets/file-icons/readme.svg")),
    ("folder", include_bytes!("../assets/file-icons/folder.svg")),
    (
        "folder-open",
        include_bytes!("../assets/file-icons/folder-open.svg"),
    ),
];

/// 已栅格化的彩色图标，按资源名索引
#[derive(Clone)]
pub struct FileIcons {
    images: HashMap<&'static str, Arc<RenderImage>>,
}

impl FileIcons {
    /// 用当前 App 的 SVG 渲染器把内置图标渲染成 2 倍图，保证高分屏下清晰
    pub fn new(cx: &App) -> Self {
        let renderer = cx.svg_renderer();
        let mut images = HashMap::new();
        for (key, bytes) in ICON_FILES {
            if let Ok(image) = renderer.render_single_frame(bytes, 2.0) {
                images.insert(*key, image);
            }
        }
        Self { images }
    }

    /// 目录树里的文件图标
    pub fn file(&self, path: &Path) -> AnyElement {
        self.element(file_key(path), IconName::File)
    }

    /// 目录树里的文件夹图标
    pub fn folder(&self, open: bool) -> AnyElement {
        let key = if open { "folder-open" } else { "folder" };
        self.element(key, IconName::Folder)
    }

    /// 标签栏的文件图标
    pub fn tab(&self, path: Option<&Path>) -> AnyElement {
        match path {
            Some(path) => self.element(file_key(path), IconName::FileText),
            None => self.element("diff", IconName::FileDiff),
        }
    }

    fn element(&self, key: &'static str, fallback: IconName) -> AnyElement {
        match self.images.get(key) {
            Some(image) => img(image.clone())
                .w(px(16.))
                .h(px(16.))
                .flex_shrink_0()
                .into_any_element(),
            // 理论上不会走到这里；万一渲染失败，退回内置的单色图标
            None => Icon::new(fallback).size_4().into_any_element(),
        }
    }
}

/// 按文件名与扩展名挑选 Material Icon Theme 里的图标
fn file_key(path: &Path) -> &'static str {
    let name = path
        .file_name()
        .map(|name| name.to_string_lossy().to_lowercase())
        .unwrap_or_default();
    let ext = path
        .extension()
        .map(|ext| ext.to_string_lossy().to_lowercase())
        .unwrap_or_default();

    if name.starts_with("readme") {
        "readme"
    } else if name.starts_with("license") || name.starts_with("licence") {
        "license"
    } else if name == ".gitignore"
        || name == ".gitattributes"
        || name == ".gitmodules"
        || name == ".gitconfig"
    {
        "git"
    } else if name == ".editorconfig" {
        "settings"
    } else {
        match ext.as_str() {
            "md" | "markdown" | "mdx" => "markdown",
            "png" | "jpg" | "jpeg" | "gif" | "webp" | "svg" | "bmp" | "ico" | "avif" => "image",
            "json" | "jsonc" => "json",
            "toml" => "toml",
            "yaml" | "yml" => "yaml",
            "lock" => "lock",
            "rs" => "rust",
            "py" | "pyi" => "python",
            "js" | "mjs" | "cjs" => "javascript",
            "jsx" => "react",
            "ts" | "mts" | "cts" => "typescript",
            "tsx" => "react_ts",
            "go" => "go",
            "java" => "java",
            "c" => "c",
            "cpp" | "cc" | "cxx" | "hpp" | "hh" | "hxx" => "cpp",
            "h" => "h",
            "sh" | "bash" | "zsh" | "fish" | "ps1" | "bat" | "cmd" => "console",
            "csv" | "tsv" | "xlsx" | "xls" => "table",
            "diff" | "patch" => "diff",
            "pdf" => "pdf",
            "doc" | "docx" | "rtf" => "word",
            "zip" | "rar" | "7z" | "tar" | "gz" | "bz2" | "xz" => "zip",
            "mp3" | "wav" | "flac" | "ogg" | "m4a" | "aac" => "audio",
            "mp4" | "mov" | "mkv" | "avi" | "webm" => "video",
            "ttf" | "otf" | "woff" | "woff2" => "font",
            "db" | "sqlite" | "sqlite3" | "sql" => "database",
            "txt" => "document",
            _ => "file",
        }
    }
}
