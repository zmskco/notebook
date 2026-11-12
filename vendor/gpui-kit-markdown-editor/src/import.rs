//! `markdown::parse` output (`BlockNode`) into the editor model.

use std::borrow::Cow;
use std::collections::HashMap;
use std::sync::Arc;

use gpui::SharedString;

use super::{
    document::{AtomData, BlockContent, EditorDocument},
    inline::{InlineAtom, InlineText},
    kind::BlockKindData,
    step::DetachedBlock,
    transaction::TransactionBuilder,
};
use gpui_kit::base::text::{
    MarkdownExtensions,
    editor_host::{BlockNode, InlineNode, LinkMark, Paragraph, TextMark, parse_markdown},
};

pub(crate) fn parse_document(
    source: &str,
    extensions: Arc<MarkdownExtensions>,
) -> Result<EditorDocument, SharedString> {
    // One line ending inside a paragraph is a line break here, not a space.
    let prepared = preserve_soft_breaks(source);
    let source = prepared.as_ref();
    let parsed = parse_markdown(source, extensions.clone())?;
    let mut doc = EditorDocument::empty(extensions);
    let root = doc.root;
    // Drop the placeholder paragraph of the empty document.
    doc.detach(root, 0);
    let mut defs = Defs::new();
    collect_definitions(&parsed, &mut defs);
    let mut blocks = Vec::new();
    for node in parsed.iter() {
        import_block(node, &defs, &mut blocks);
    }
    for (ix, block) in blocks.into_iter().enumerate() {
        doc.attach(root, ix, block);
    }
    // Same structural rules as after every transaction (empty list items
    // get a paragraph, an empty document gets one leaf). The steps of this
    // pass are not history.
    let mut tx = TransactionBuilder::new(&mut doc);
    tx.normalize()
        .map_err(|e| SharedString::from(format!("{e:?}")))?;
    drop(tx);
    doc.structure_dirty = true;
    doc.refresh_order();
    // Markdown parsers omit final blank paragraphs. Reconstruct that editable
    // boundary as real document nodes, never a mutable raw-source shadow.
    // Primary AST spans distinguish exterior footer trivia from line endings
    // owned by an unterminated code fence/body.
    let exterior_start = parsed
        .last()
        .and_then(BlockNode::span)
        .map_or(0, |span| span.end);
    let exterior = source.get(exterior_start..).unwrap_or_default();
    let trimmed_spaces = exterior.trim_end_matches([' ', '\t']);
    let stripped = trimmed_spaces.trim_end_matches(['\r', '\n']);
    let line_ends = &trimmed_spaces[stripped.len()..];
    let ending_count = if line_ends.contains('\n') {
        line_ends.bytes().filter(|b| *b == b'\n').count()
    } else {
        line_ends.len()
    };
    for _ in 0..ending_count.div_ceil(2) {
        let index = doc.children(root).len();
        doc.attach(
            root,
            index,
            DetachedBlock::leaf(
                BlockKindData::Paragraph,
                BlockContent::Inline(InlineText::default()),
            ),
        );
    }
    doc.refresh_order();

    // CommonMark ignores terminal paragraph spaces, but in a live editing
    // buffer they separate the next typed/dictated word. Restore that trivia
    // into the document, not a second mutable source buffer. Code retains its
    // exact own body and must not inherit trivia after its closing fence.
    let trimmed = source.trim_end_matches([' ', '\t']);
    let mut tail = &source[trimmed.len()..];
    let last = *doc.leaves().last().unwrap();
    if (matches!(doc.kind(last), Some(BlockKindData::Heading { .. }))
        || doc.index_in_parent(last).is_some_and(|(parent, _)| {
            matches!(doc.kind(parent), Some(BlockKindData::ListItem { .. }))
        }))
        && doc.inline(last).is_some_and(|inline| inline.is_empty())
    {
        // One space is the empty heading's structural marker separator.
        tail = tail.strip_prefix(' ').unwrap_or(tail);
    }
    if !tail.is_empty()
        && let Some(entry) = doc.get_mut(last)
        && let BlockContent::Inline(inline) = &mut entry.content
    {
        inline.push(InlineText::plain(tail, TextMark::default()));
    }
    doc.revision = 0;
    Ok(doc)
}

fn leaf(kind: BlockKindData, content: BlockContent) -> DetachedBlock {
    DetachedBlock::leaf(kind, content)
}

fn import_children(nodes: &[BlockNode], defs: &Defs) -> Vec<DetachedBlock> {
    let mut out = Vec::new();
    for node in nodes {
        import_block(node, defs, &mut out);
    }
    out
}

/// Recover literal fenced code inside a paragraph. A plain-text paste or a
/// soft line break leaves ```` ```lang ```` as prose, and the serializer
/// escapes those line starts so the parser keeps them literal; split such a
/// paragraph into the prose and code blocks the unescaped text would parse
/// to (an unclosed fence runs to the end of the paragraph).
/// Returns `None` when the paragraph holds no fence line.
fn paragraph_with_fences(inline: &InlineText) -> Option<Vec<DetachedBlock>> {
    let text = inline.text();
    // An inline atom cannot live in a code block's plain body.
    if !text.contains('\n') || text.contains(super::inline::ATOM_CHAR) {
        return None;
    }
    let mut blocks = Vec::new();
    let mut prose_start = 0;
    // (fence character, opening length, body start, language)
    let mut open: Option<(char, usize, usize, Option<SharedString>)> = None;
    let mut found = false;
    let mut line_start = 0;
    loop {
        let line_end = text[line_start..]
            .find('\n')
            .map_or(text.len(), |ix| line_start + ix);
        let line = &text[line_start..line_end];
        match &open {
            None => {
                // A fence line needs at least one more line (body or closing).
                if line_end < text.len()
                    && let Some((fence, len, info)) = split_fence(line)
                {
                    push_prose(&mut blocks, inline, prose_start, line_start);
                    let lang = info.split_whitespace().next().map(SharedString::from);
                    open = Some((fence, len, line_end + 1, lang));
                    found = true;
                }
            }
            Some((fence, len, body_start, lang)) => {
                if is_closing_fence(line, *fence, *len) {
                    // The line break before the closing fence is not body text.
                    let end = line_start.saturating_sub(1).max(*body_start);
                    push_code(&mut blocks, text, *body_start, end, lang.clone());
                    open = None;
                    prose_start = line_end + 1;
                }
            }
        }
        if line_end == text.len() {
            break;
        }
        line_start = line_end + 1;
    }
    if let Some((_, _, body_start, lang)) = open {
        push_code(&mut blocks, text, body_start, text.len(), lang);
        prose_start = text.len();
    }
    push_prose(&mut blocks, inline, prose_start, text.len());
    found.then_some(blocks)
}

fn push_prose(blocks: &mut Vec<DetachedBlock>, inline: &InlineText, start: usize, end: usize) {
    // Drop the soft break that separated the prose from its fence.
    let end = match inline.text().get(..end) {
        Some(prose) if prose.ends_with('\n') => end - 1,
        _ => end,
    };
    if start >= end {
        return;
    }
    blocks.push(leaf(
        BlockKindData::Paragraph,
        BlockContent::Inline(inline.slice(start..end)),
    ));
}

/// Push the code block whose body is `text[body_start..body_end]`.
fn push_code(
    blocks: &mut Vec<DetachedBlock>,
    text: &str,
    body_start: usize,
    body_end: usize,
    lang: Option<SharedString>,
) {
    let end = body_end.min(text.len()).max(body_start);
    let body = text[body_start.min(end)..end].to_string();
    blocks.push(leaf(
        BlockKindData::CodeBlock { lang, meta: None },
        BlockContent::Plain(body),
    ));
}

/// Opening fence: three or more backticks or tildes and an optional info
/// string (a backtick fence's info cannot contain a backtick).
fn split_fence(line: &str) -> Option<(char, usize, &str)> {
    let line = line.trim_end_matches([' ', '\t', '\r']);
    let fence = line.chars().next()?;
    if fence != '`' && fence != '~' {
        return None;
    }
    let len = line.chars().take_while(|ch| *ch == fence).count();
    if len < 3 {
        return None;
    }
    let info = line[len..].trim();
    if fence == '`' && info.contains('`') {
        return None;
    }
    Some((fence, len, info))
}

/// Closing fence: the same character, at least as long as the opening one and
/// nothing else on the line but trailing spaces.
fn is_closing_fence(line: &str, fence: char, len: usize) -> bool {
    let line = line.trim_end_matches([' ', '\t', '\r']);
    let run = line.chars().take_while(|ch| *ch == fence).count();
    run >= len && line.chars().count() == run
}

pub(crate) fn import_block(node: &BlockNode, defs: &Defs, out: &mut Vec<DetachedBlock>) {
    let block = match node {
        // An HTML container: splice its children in place.
        BlockNode::Root { children, .. } => {
            for child in children {
                import_block(child, defs, out);
            }
            return;
        }
        BlockNode::Paragraph(paragraph) => {
            let inline = import_inline(paragraph, defs);
            // Literal fences (plain-text paste, soft line breaks) become code
            // blocks, so a note saved that way reopens highlighted.
            if let Some(blocks) = paragraph_with_fences(&inline) {
                out.extend(blocks);
                return;
            }
            leaf(BlockKindData::Paragraph, BlockContent::Inline(inline))
        }
        BlockNode::Heading {
            level, children, ..
        } => leaf(
            BlockKindData::Heading { level: *level },
            BlockContent::Inline(import_inline(children, defs)),
        ),
        BlockNode::Blockquote { children, .. } => {
            DetachedBlock::container(BlockKindData::Blockquote, import_children(children, defs))
        }
        BlockNode::List {
            children,
            ordered,
            start,
            ..
        } => DetachedBlock::container(
            BlockKindData::List {
                ordered: *ordered,
                start: *start,
            },
            import_children(children, defs),
        ),
        BlockNode::ListItem {
            children,
            spread,
            checked,
            ..
        } => {
            let mut children = import_children(children, defs);
            // A parsed bare marker is an editable empty item, not an empty
            // container to prune during normalization.
            if children.is_empty() {
                children.push(DetachedBlock::paragraph(InlineText::new()));
            }
            DetachedBlock::container(
                BlockKindData::ListItem {
                    checked: *checked,
                    spread: *spread,
                },
                children,
            )
        }
        BlockNode::CodeBlock(code) => {
            let text = code.code().to_string();
            leaf(
                BlockKindData::CodeBlock {
                    lang: code.lang(),
                    meta: code.meta(),
                },
                BlockContent::Plain(text),
            )
        }
        BlockNode::Custom { node, .. } => leaf(
            BlockKindData::Custom,
            BlockContent::Atom(AtomData::Custom(node.clone())),
        ),
        BlockNode::Table(table) => {
            let rows = table
                .rows()
                .map(|row| {
                    DetachedBlock::container(
                        BlockKindData::TableRow,
                        row.map(|cell| {
                            leaf(
                                BlockKindData::TableCell,
                                BlockContent::Inline(import_inline(cell, defs)),
                            )
                        })
                        .collect(),
                    )
                })
                .collect();
            DetachedBlock::container(
                BlockKindData::Table {
                    aligns: table.column_aligns().to_vec(),
                },
                rows,
            )
        }
        BlockNode::Break { html, .. } => leaf(
            BlockKindData::Break { html: *html },
            BlockContent::Atom(AtomData::None),
        ),
        BlockNode::HorizontalRule { .. } => leaf(
            BlockKindData::HorizontalRule,
            BlockContent::Atom(AtomData::None),
        ),
        BlockNode::Definition {
            identifier,
            url,
            title,
            ..
        } => leaf(
            BlockKindData::Definition {
                identifier: identifier.clone(),
                url: url.clone(),
                title: title.clone(),
            },
            BlockContent::Atom(AtomData::None),
        ),
        BlockNode::Unknown | _ => leaf(BlockKindData::Unknown, BlockContent::Atom(AtomData::None)),
    };
    out.push(block);
}

/// Convert a paragraph's inline nodes into normalized runs. `InlineNode.marks`
/// may overlap (`**b*c*d**` gives bold 0..3 and italic 1..2), so split at
/// every boundary and merge the marks covering each piece.
///
/// Reference links (`[text][id]`) arrive with an empty url and an
/// identifier; they are resolved against the document's definitions so the
/// canonical output is an inline link. Unresolved references (and footnote
/// links) lose their link mark: documented lossiness.
pub(crate) fn import_inline(paragraph: &Paragraph, defs: &Defs) -> InlineText {
    let mut out = InlineText::new();
    for node in paragraph.children() {
        out.push(import_inline_node(node));
    }
    let len = out.len();
    out.map_marks(0..len, |mark| match &mark.link {
        Some(link) if link.url.is_empty() => {
            let link = link
                .identifier
                .as_ref()
                .and_then(|id| defs.get(&SharedString::from(id.to_lowercase())))
                .map(|(url, title)| LinkMark {
                    url: url.clone(),
                    identifier: None,
                    title: title.clone(),
                });
            TextMark {
                link,
                ..mark.clone()
            }
        }
        _ => mark.clone(),
    });
    out
}

pub(crate) type Defs = HashMap<SharedString, (SharedString, Option<SharedString>)>;

pub(crate) fn collect_definitions(nodes: &[BlockNode], defs: &mut Defs) {
    for node in nodes {
        match node {
            BlockNode::Definition {
                identifier,
                url,
                title,
                ..
            } => {
                defs.insert(
                    identifier.to_lowercase().into(),
                    (url.clone(), title.clone()),
                );
            }
            BlockNode::Root { children, .. }
            | BlockNode::Blockquote { children, .. }
            | BlockNode::List { children, .. }
            | BlockNode::ListItem { children, .. } => collect_definitions(children, defs),
            _ => {}
        }
    }
}

fn import_inline_node(node: &InlineNode) -> InlineText {
    let merged_marks = || {
        let mut mark = TextMark::default();
        for (_, layer) in node.mark_runs() {
            mark.merge(layer.clone());
        }
        mark
    };
    if let Some(custom) = node.as_custom() {
        return InlineText::atom(InlineAtom::Custom(custom.clone()), merged_marks());
    }
    if let Some(image) = node.as_image() {
        return InlineText::atom(InlineAtom::Image(image.clone()), merged_marks());
    }
    let text: &str = node.text();
    let mut cuts = vec![0, text.len()];
    for (range, _) in node.mark_runs() {
        cuts.push(range.start.min(text.len()));
        cuts.push(range.end.min(text.len()));
    }
    cuts.sort_unstable();
    cuts.dedup();
    let mut out = InlineText::new();
    for pair in cuts.windows(2) {
        let (lo, hi) = (pair[0], pair[1]);
        if lo >= hi || !text.is_char_boundary(lo) || !text.is_char_boundary(hi) {
            continue;
        }
        let mut mark = TextMark::default();
        for (range, layer) in node.mark_runs() {
            if range.start <= lo && hi <= range.end {
                mark.merge(layer.clone());
            }
        }
        out.push(InlineText::plain(&text[lo..hi], mark));
    }
    out
}

/// CommonMark folds a single line ending inside a paragraph into a space, but
/// this editor treats it as a line break. Give every prose soft break the
/// two-space hard-break form first: the parser then keeps it as a `\n` in the
/// paragraph, and the serializer writes that back out as one bare newline.
fn preserve_soft_breaks(source: &str) -> Cow<'_, str> {
    use ::markdown::mdast::Node;

    fn collect(node: &Node, source: &str, prose: bool, out: &mut Vec<usize>) {
        // Blocks whose line endings are content or structure, not prose.
        let prose = prose
            && !matches!(
                node,
                Node::Heading(_) | Node::Code(_) | Node::Html(_) | Node::Definition(_)
            );
        if let Node::Text(_) = node {
            if prose
                && let Some(span) = node.position()
                && let Some(slice) = source.get(span.start.offset..span.end.offset)
            {
                for (ix, _) in slice.match_indices('\n') {
                    out.push(span.start.offset + ix);
                }
            }
            return;
        }
        if let Some(children) = node.children() {
            for child in children {
                collect(child, source, prose, out);
            }
        }
    }

    let Ok(root) = ::markdown::to_mdast(source, &::markdown::ParseOptions::gfm()) else {
        return Cow::Borrowed(source);
    };
    let mut breaks = Vec::new();
    collect(&root, source, true, &mut breaks);
    breaks.sort_unstable();
    breaks.dedup();
    // A line that already ends in a hard break needs no second one, and a line
    // ending at a blank line ends a paragraph: nothing to preserve there. A
    // break at the very end of the source does count: it is the trailing empty
    // line of the last paragraph.
    breaks.retain(|&at| {
        let before = &source[..at];
        let line = before.rsplit('\n').next().unwrap_or_default();
        let rest = &source[at + 1..];
        let after = rest.split('\n').next().unwrap_or_default();
        !line.trim_end().is_empty()
            && !line.ends_with("  ")
            && (rest.is_empty() || !after.trim().is_empty())
    });
    if breaks.is_empty() {
        return Cow::Borrowed(source);
    }
    let mut out = String::with_capacity(source.len() + breaks.len() * 2);
    let mut last = 0;
    for at in breaks {
        out.push_str(&source[last..at]);
        out.push_str("  ");
        last = at;
    }
    out.push_str(&source[last..]);
    Cow::Owned(out)
}

/// Footnotes and unsupported HTML are outside the editable block vocabulary. Keep their original
/// source in opaque, selectable atoms rather than accepting the display
/// parser's decorated-prose projection. Uses the same AST/parser pipeline as
/// every other editor node; no second parser or mutable source mirror.
pub(crate) fn preserve_source_extensions(extensions: MarkdownExtensions) -> MarkdownExtensions {
    use ::markdown::mdast::Node;
    use gpui_kit::base::text::{MarkdownNode, MarkdownParseContext, MarkdownPlugin};

    struct SourceAtom {
        block: bool,
    }
    impl MarkdownPlugin for SourceAtom {
        fn name(&self) -> &str {
            if self.block {
                "editor-source-block"
            } else {
                "editor-source-inline"
            }
        }
        fn is_block(&self) -> bool {
            self.block
        }
        fn parse(&self, node: &Node, cx: &MarkdownParseContext<'_>) -> Option<MarkdownNode> {
            let known_html = if let Node::Html(html) = node {
                let lower = html.value.to_ascii_lowercase();
                let tag = lower
                    .trim_start_matches('<')
                    .trim_start_matches('/')
                    .split(|ch: char| !ch.is_ascii_alphabetic())
                    .next()
                    .unwrap_or_default();
                matches!(tag, "u" | "mark" | "em" | "strong" | "s" | "del" | "br")
            } else {
                false
            };
            let accepts = if self.block {
                matches!(node, Node::FootnoteDefinition(_))
                    || (matches!(node, Node::Html(_)) && !known_html)
            } else {
                matches!(node, Node::FootnoteReference(_))
                    || (matches!(node, Node::Html(_)) && !known_html)
            };
            if !accepts {
                return None;
            }
            let source = cx.node_source(node)?;
            // Block spans may include separator line endings. The root owns
            // those boundaries; retaining them here adds a blank line on every
            // export/import cycle. Internal content remains verbatim.
            let source = if self.block {
                source.trim_end_matches(['\r', '\n'])
            } else {
                source
            };
            let source = if self.block {
                // The AST span starts after the first container marker, but
                // later source lines still include it. Strip only matching
                // continuation prefixes, preserving body indentation and lazy
                // lines. The root serializer will add the containers once.
                let start = node.position()?.start.offset;
                let line_start = cx.source()[..start].rfind('\n').map_or(0, |ix| ix + 1);
                let prefix = &cx.source()[line_start..start];
                let continuation = prefix
                    .chars()
                    .map(|ch| if ch == '>' { '>' } else { ' ' })
                    .collect::<String>();
                source
                    .split('\n')
                    .enumerate()
                    .map(|(ix, line)| {
                        if ix == 0 || continuation.is_empty() {
                            line
                        } else if line.trim_end() == continuation.trim_end() {
                            ""
                        } else {
                            line.strip_prefix(&continuation).unwrap_or(line)
                        }
                    })
                    .collect::<Vec<_>>()
                    .join("\n")
            } else {
                source.to_owned()
            };
            Some(
                MarkdownNode::new(self.name(), ())
                    .text(source.clone())
                    .markdown(source),
            )
        }
    }
    extensions
        .plugin(SourceAtom { block: true })
        .plugin(SourceAtom { block: false })
}
