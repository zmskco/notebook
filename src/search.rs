//! 全局搜索：在工作区里查找文本、批量替换

use std::path::{Path, PathBuf};

/// 一行里的第一处命中
pub struct Hit {
    /// 行号，从 1 开始
    pub line: usize,
    /// 整行文本
    pub text: String,
    /// 命中在行内的字节区间
    pub start: usize,
    pub end: usize,
}

/// 一个文件里的命中
pub struct FileHits {
    pub path: PathBuf,
    pub hits: Vec<Hit>,
}

pub struct Outcome {
    pub files: Vec<FileHits>,
    pub total: usize,
    pub truncated: bool,
}

/// 结果太多时截断，避免一次搜索把界面撑满
const MAX_FILES: usize = 300;
const MAX_HITS: usize = 2000;
const MAX_HITS_PER_FILE: usize = 200;
/// 超过这个大小的文件基本不是笔记，直接跳过
const MAX_FILE_BYTES: u64 = 2 * 1024 * 1024;

/// 收集工作区里的所有文件，跳过隐藏项（.git 等）
pub fn collect_files(dir: &Path, out: &mut Vec<PathBuf>) {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    let mut dirs = Vec::new();
    let mut files = Vec::new();
    for entry in entries.flatten() {
        if entry.file_name().to_string_lossy().starts_with('.') {
            continue;
        }
        let path = entry.path();
        if path.is_dir() {
            dirs.push(path);
        } else {
            files.push(path);
        }
    }
    dirs.sort();
    files.sort();
    for dir in dirs {
        collect_files(&dir, out);
    }
    out.extend(files);
}

/// 在工作区里查找 needle，每个文件记录每行的第一处命中
pub fn search(root: &Path, needle: &str, case_sensitive: bool) -> Outcome {
    let mut paths = Vec::new();
    collect_files(root, &mut paths);

    let mut files = Vec::new();
    let mut total = 0;
    let mut truncated = false;

    for path in paths {
        if total >= MAX_HITS || files.len() >= MAX_FILES {
            truncated = true;
            break;
        }
        if std::fs::metadata(&path).map(|meta| meta.len()).unwrap_or(0) > MAX_FILE_BYTES {
            continue;
        }
        // 读不出 UTF-8 的多半是二进制文件，直接跳过
        let Ok(text) = std::fs::read_to_string(&path) else {
            continue;
        };

        let mut hits = Vec::new();
        for (ix, line) in text.lines().enumerate() {
            let Some((start, end)) = find(line, needle, case_sensitive) else {
                continue;
            };
            hits.push(Hit {
                line: ix + 1,
                text: line.to_string(),
                start,
                end,
            });
            total += 1;
            if hits.len() >= MAX_HITS_PER_FILE || total >= MAX_HITS {
                truncated = true;
                break;
            }
        }
        if !hits.is_empty() {
            files.push(FileHits { path, hits });
        }
    }

    Outcome {
        files,
        total,
        truncated,
    }
}

/// 找 needle 在 hay 里的第一处位置，返回字节区间
pub fn find(hay: &str, needle: &str, case_sensitive: bool) -> Option<(usize, usize)> {
    if needle.is_empty() {
        return None;
    }
    if case_sensitive {
        hay.find(needle).map(|start| (start, start + needle.len()))
    } else {
        find_ci(hay, needle)
    }
}

/// 不分大小写地查找，返回原始字符串里的字节区间
fn find_ci(hay: &str, needle: &str) -> Option<(usize, usize)> {
    let first: String = needle.chars().next()?.to_lowercase().collect();
    for (start, ch) in hay.char_indices() {
        if ch.to_lowercase().collect::<String>() != first {
            continue;
        }
        let mut end = start;
        let mut matched = true;
        let mut chars = hay[start..].chars();
        for nc in needle.chars() {
            let Some(hc) = chars.next() else {
                matched = false;
                break;
            };
            if hc.to_lowercase().collect::<String>() != nc.to_lowercase().collect::<String>() {
                matched = false;
                break;
            }
            end += hc.len_utf8();
        }
        if matched {
            return Some((start, end));
        }
    }
    None
}

/// 查找 needle 在 hay 里的全部位置，返回字节区间
pub fn occurrences(hay: &str, needle: &str, case_sensitive: bool) -> Vec<(usize, usize)> {
    let mut ranges = Vec::new();
    if needle.is_empty() {
        return ranges;
    }
    let mut rest = hay;
    let mut offset = 0;
    while let Some((start, end)) = find(rest, needle, case_sensitive) {
        ranges.push((offset + start, offset + end));
        // 从命中结尾继续找；needle 非空，end 必然大于 start
        rest = &rest[end..];
        offset += end;
    }
    ranges
}

/// 替换文本里的全部命中，返回新文本与替换次数
pub fn replace(text: &str, needle: &str, replacement: &str, case_sensitive: bool) -> (String, usize) {
    if needle.is_empty() {
        return (text.to_string(), 0);
    }
    if case_sensitive {
        let count = text.matches(needle).count();
        if count == 0 {
            return (text.to_string(), 0);
        }
        (text.replace(needle, replacement), count)
    } else {
        let mut out = String::with_capacity(text.len());
        let mut rest = text;
        let mut count = 0;
        while let Some((start, end)) = find_ci(rest, needle) {
            out.push_str(&rest[..start]);
            out.push_str(replacement);
            rest = &rest[end..];
            count += 1;
        }
        if count == 0 {
            return (text.to_string(), 0);
        }
        out.push_str(rest);
        (out, count)
    }
}
