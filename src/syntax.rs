//! 代码块语法高亮配色：复用 Dracula 官方主题的色板。
//!
//! Dracula Theme Official 是 VS Code 的配色扩展，本身不能被直接加载；但它调的只是
//! 一组颜色，这里把它们按 Zed 高亮主题的字段名写下来，解析成 [`HighlightTheme`]。
//! 深色用 Dracula，浅色用同一作者的官方浅色变体 Alucard，保证浅底上仍然看得清。

use std::sync::Arc;

use gpui_kit::component::highlighter::HighlightTheme;

/// Dracula：背景 #282a36，前景 #f8f8f2，注释 #6272a4
const DRACULA: &str = r##"{
  "name": "Dracula",
  "appearance": "dark",
  "style": {
    "editor.background": "#282a36",
    "editor.foreground": "#f8f8f2",
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

/// Alucard：Dracula 的官方浅色变体
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

/// 按主题明暗取对应的高亮配色
pub fn highlight_theme(dark: bool) -> Arc<HighlightTheme> {
    let json = if dark { DRACULA } else { ALUCARD };
    let theme: HighlightTheme =
        serde_json::from_str(json).expect("内置的高亮主题 JSON 格式有误");
    Arc::new(theme)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn both_variants_parse() {
        for dark in [true, false] {
            let theme = highlight_theme(dark);
            assert_eq!(theme.name, if dark { "Dracula" } else { "Alucard" });
            assert!(theme.style.syntax.keyword.is_some());
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
