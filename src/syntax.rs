//! 代码块语法高亮配色：复用 Dracula 官方主题的色板。
//!
//! Dracula Theme Official 是 VS Code 的配色扩展，本身不能被直接加载；但它调的只是
//! 一组颜色，这里把它们按 Zed 高亮主题的字段名写下来，解析成 [`HighlightTheme`]。
//! 深色用 Dracula，浅色用同一作者的官方浅色变体 Alucard，保证浅底上仍然看得清。

use std::sync::Arc;

use gpui_kit::base::text::{CodeBlock, TextViewDefaults, TextViewStyle};
use gpui_kit::component::highlighter::{HighlightTheme, LanguageRegistry, SyntaxHighlighter};
use gpui_kit::Refineable;

/// Dracula：背景 #282a36，前景 #f8f8f2，注释 #6272a4
const DRACULA: &str = r##"{
  "name": "Dracula",
  "appearance": "dark",
  "style": {
    "editor.background": "#282a36",
    "editor.foreground": "#f8f8f2",
    "editor.gutter.background": "#282a36",
    "syntax": {
      "attribute": { "color": "#50fa7b", "font_style": "italic" },
      "boolean": { "color": "#bd93f9" },
      "comment": { "color": "#6272a4", "font_style": "italic" },
      "comment_doc": { "color": "#6272a4", "font_style": "italic" },
      "constant": { "color": "#bd93f9" },
      "constructor": { "color": "#50fa7b" },
      "embedded": { "color": "#ffb86c" },
      "emphasis": { "color": "#f8f8f2", "font_style": "italic" },
      "emphasis.strong": { "color": "#f8f8f2", "font_weight": 700 },
      "enum": { "color": "#8be9fd" },
      "function": { "color": "#50fa7b" },
      "hint": { "color": "#6272a4" },
      "keyword": { "color": "#ff79c6" },
      "label": { "color": "#8be9fd" },
      "link_text": { "color": "#8be9fd" },
      "link_uri": { "color": "#ff79c6", "font_style": "underline" },
      "number": { "color": "#bd93f9" },
      "operator": { "color": "#ff79c6" },
      "predictive": { "color": "#6272a4", "font_style": "italic" },
      "preproc": { "color": "#ff79c6" },
      "primary": { "color": "#f8f8f2" },
      "property": { "color": "#f8f8f2" },
      "punctuation": { "color": "#f8f8f2" },
      "punctuation.bracket": { "color": "#f8f8f2" },
      "punctuation.delimiter": { "color": "#f8f8f2" },
      "punctuation.list_marker": { "color": "#ff79c6" },
      "punctuation.special": { "color": "#ff79c6" },
      "string": { "color": "#f1fa8c" },
      "string.escape": { "color": "#ff79c6" },
      "string.regex": { "color": "#ff5555" },
      "string.special": { "color": "#ff79c6" },
      "string.special.symbol": { "color": "#ff79c6" },
      "tag": { "color": "#ff79c6" },
      "tag.doctype": { "color": "#6272a4" },
      "text.code.span": { "color": "#ffb86c" },
      "text.literal": { "color": "#f1fa8c" },
      "title": { "color": "#8be9fd", "font_weight": 700 },
      "type": { "color": "#8be9fd" },
      "variable": { "color": "#f8f8f2" },
      "variable.special": { "color": "#ffb86c", "font_style": "italic" },
      "variant": { "color": "#8be9fd" }
    }
  }
}"##;

/// Alucard：Dracula 作者的官方浅色变体，浅色主题用
const ALUCARD: &str = r##"{
  "name": "Alucard",
  "appearance": "light",
  "style": {
    "editor.background": "#fffbed",
    "editor.foreground": "#1f1f1f",
    "syntax": {
      "attribute": { "color": "#14710a", "font_style": "italic" },
      "boolean": { "color": "#644ac9" },
      "comment": { "color": "#6c664b", "font_style": "italic" },
      "comment_doc": { "color": "#6c664b", "font_style": "italic" },
      "constant": { "color": "#644ac9" },
      "constructor": { "color": "#14710a" },
      "embedded": { "color": "#a34d14" },
      "emphasis": { "color": "#1f1f1f", "font_style": "italic" },
      "emphasis.strong": { "color": "#1f1f1f", "font_weight": 700 },
      "enum": { "color": "#036a96" },
      "function": { "color": "#14710a" },
      "hint": { "color": "#6c664b" },
      "keyword": { "color": "#a3144d" },
      "label": { "color": "#036a96" },
      "link_text": { "color": "#036a96" },
      "link_uri": { "color": "#a3144d", "font_style": "underline" },
      "number": { "color": "#644ac9" },
      "operator": { "color": "#a3144d" },
      "predictive": { "color": "#6c664b", "font_style": "italic" },
      "preproc": { "color": "#a3144d" },
      "primary": { "color": "#1f1f1f" },
      "property": { "color": "#1f1f1f" },
      "punctuation": { "color": "#1f1f1f" },
      "punctuation.bracket": { "color": "#1f1f1f" },
      "punctuation.delimiter": { "color": "#1f1f1f" },
      "punctuation.list_marker": { "color": "#a3144d" },
      "punctuation.special": { "color": "#a3144d" },
      "string": { "color": "#846e15" },
      "string.escape": { "color": "#a3144d" },
      "string.regex": { "color": "#cb3a2a" },
      "string.special": { "color": "#a3144d" },
      "string.special.symbol": { "color": "#a3144d" },
      "tag": { "color": "#a3144d" },
      "tag.doctype": { "color": "#6c664b" },
      "text.code.span": { "color": "#a34d14" },
      "text.literal": { "color": "#846e15" },
      "title": { "color": "#036a96", "font_weight": 700 },
      "type": { "color": "#036a96" },
      "variable": { "color": "#1f1f1f" },
      "variable.special": { "color": "#a34d14", "font_style": "italic" },
      "variant": { "color": "#036a96" }
    }
  }
}"##;

/// 源码视图（含代码文件）与代码块的语法高亮：Dracula Theme Official 家族配色，
/// 深色主题用 Dracula（黑色版），浅色主题用官方浅色变体 Alucard，各自对应。
pub fn highlight_theme(dark: bool) -> Arc<HighlightTheme> {
    parse_theme(if dark { DRACULA } else { ALUCARD })
}

fn parse_theme(json: &str) -> Arc<HighlightTheme> {
    let theme: HighlightTheme =
        serde_json::from_str(json).expect("内置的高亮主题 JSON 格式有误");
    Arc::new(theme)
}

/// 安装代码块语法高亮与配套配色，覆盖所见即所得 / 演示 / 对照里渲染出的代码块。
///
/// 每次应用主题都要重新安装：代码块的配色按「高亮函数」缓存，
/// 换一个函数（Arc 指针不同）就会让旧配色失效，按新主题重新着色。
pub fn install(dark: bool, cx: &mut gpui_kit::App) {
    let theme = highlight_theme(dark);
    let code_style = pin_code_colors(
        TextViewStyle::from_theme(&gpui_kit::base::Theme::global(cx)),
        dark,
    );
    TextViewDefaults::new()
        .with_style(code_style)
        .with_code_block_highlighter(move |block| highlight_block(block, &theme))
        .install(cx);
}

/// 代码块 / 内联代码 / 表格头的配色，随应用主题取家族变体：
/// 深色 Dracula（黑底白字、橙色内联代码），浅色 Alucard（米白底深字、橙棕内联代码）
fn code_palette(dark: bool) -> (u32, u32, u32) {
    if dark {
        (0x282A36, 0xF8F8F2, 0xFFB86C)
    } else {
        (0xFFFbed, 0x1F1F1F, 0xA34D14)
    }
}

/// 把样式里「代码相关」的颜色钉成对应变体配色：围栏代码块、内联代码、表格头。
///
/// 其余颜色（正文、标题、链接…）仍从传入的主题样式继承。
fn pin_code_colors(style: TextViewStyle, dark: bool) -> TextViewStyle {
    let (bg_hex, fg_hex, inline_hex) = code_palette(dark);
    let bg = gpui_kit::rgb(bg_hex).into();
    let fg = gpui_kit::rgb(fg_hex).into();
    let inline = gpui_kit::rgb(inline_hex).into();
    // 在主题默认值之上只覆盖颜色，避免丢掉内置的字体等设置
    let fg_refine = gpui_kit::StyleRefinement {
        text: gpui_kit::TextStyleRefinement {
            color: Some(fg),
            ..Default::default()
        },
        ..Default::default()
    };
    let inline_refine = gpui_kit::StyleRefinement {
        text: gpui_kit::TextStyleRefinement {
            color: Some(inline),
            ..Default::default()
        },
        ..Default::default()
    };

    let mut code_block = style.code_block().clone();
    code_block.refine(&fg_refine);
    let mut table_head = style.table_head().clone();
    table_head.refine(&fg_refine);
    let mut inline_code_style = style.inline_code_style().clone();
    inline_code_style.refine(&inline_refine);
    let mut inline_code = style.inline_code().clone();
    inline_code.color = Some(inline);
    inline_code.background_color = Some(bg);

    style
        .with_code_background(bg)
        .with_code_block(code_block)
        .with_table_head(table_head)
        .with_inline_code_style(inline_code_style)
        .with_inline_code(inline_code)
}

/// 把一个代码块解析成「字节区间 → 高亮样式」列表；无语言标签或语言未注册时保持纯文本。
fn highlight_block(
    block: &CodeBlock,
    theme: &HighlightTheme,
) -> Vec<(std::ops::Range<usize>, gpui_kit::HighlightStyle)> {
    let Some(lang) = block.lang() else {
        return Vec::new();
    };
    let lang = normalize_language(&lang);
    // 未注册的语言会退化成纯文本；这里直接跳过，也省得每次都建一次解析器
    if lang == "text" || LanguageRegistry::singleton().language(&lang).is_none() {
        return Vec::new();
    }
    let code = block.code();
    let mut highlighter = SyntaxHighlighter::new(&lang);
    let rope = ropey::Rope::from_str(&code);
    highlighter.update(None, &rope, None);
    highlighter.styles(&(0..code.len()), theme)
}

/// 把围栏信息串的常见写法归一化成 tree-sitter 语法名（取第一个词、转小写）。
///
/// gpui-kit 自带的别名（js / py / ts / sh / c++ / yml 等）之外，
/// 这里补上几个常见的写法。
fn normalize_language(tag: &str) -> String {
    let tag = tag
        .trim()
        .to_ascii_lowercase()
        .split([',', ' ', '\t'])
        .next()
        .unwrap_or("")
        .to_string();
    match tag.as_str() {
        "" | "text" | "txt" | "plain" | "plaintext" => "text".into(),
        "shell" => "bash".into(),
        "golang" => "go".into(),
        "jsx" => "javascript".into(),
        // HTML 语法解析 XML 足够接近，比纯文本强
        "xml" => "html".into(),
        other => other.into(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn both_variants_parse() {
        for (dark, name) in [(true, "Dracula"), (false, "Alucard")] {
            let theme = highlight_theme(dark);
            assert_eq!(theme.name, name);
            assert!(theme.style.syntax.keyword.is_some());
            // 编辑器表面字段齐全：输入框表面跟着主题（黑底 / 米白底）
            assert!(theme.style.editor_background.is_some());
        }
    }

    /// 确认 tree-sitter-languages 特性真的把 Rust 语法带进来了，
    /// 否则代码块会退化成纯文本，主题再怎么配也不会生效。
    #[test]
    fn rust_grammar_is_registered() {
        let config = gpui_kit::component::highlighter::LanguageRegistry::singleton()
            .language("rust")
            .expect("Rust 语法未注册");
        assert!(config.has_grammar());
    }
}
