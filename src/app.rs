//! 主界面：侧栏（资源管理器 / 搜索 / 源代码管理）+ 多标签编辑区 + 状态栏

use std::borrow::Cow;
use std::collections::HashSet;
use std::path::{Path, PathBuf};

use gpui_kit::assets::IconName;
use gpui_kit::component::button::{Button, ButtonVariants};
use gpui_kit::component::dialog::{DialogAction, DialogFooter};
use gpui_kit::component::diff::{Diff, DiffFile, DiffState};
use gpui_kit::component::input::{Editor, EditorState, Input, InputEvent, InputState, Position};
use gpui_kit::component::list::ListItem;
use gpui_kit::component::menu::{ContextMenuExt, DropdownMenu, PopupMenuItem};
use gpui_kit::base::text::{RangeHighlight, TextViewState};
use gpui_kit::component::text::TextView;
use gpui_kit::component::scroll::ScrollableElement;
use gpui_kit::component::tree::{tree, TreeEvent, TreeState};
use gpui_kit::component::{
    ActiveTheme, Disableable, Icon, Selectable, Sizable, StyledExt, Theme, ThemeMode, TitleBar,
    WindowExt, h_flex, v_flex,
};
use gpui_kit::*;
use markdown_editor::{
    BlockEditor, BlockEditorEvent, BlockEditorState, BlockId, BlockPosition, BlockSelection,
};

use crate::file_icons::FileIcons;
use crate::fs_ops;
use crate::git;
use crate::search;
use crate::{
    CloseTab, DeleteSelected, FocusFind, FocusSearch, InlineCancel, InlineCommit, NextTab,
    OpenReplace, PrevTab, Refresh, RenameSelected, Save, TogglePreview,
};

#[derive(Clone, Copy, PartialEq, Eq)]
enum InlineKind {
    NewFile,
    NewFolder,
    Rename,
}

/// 目录树里的内联输入：新建文件和文件夹、重命名都用它在目标位置直接改名，不弹窗
struct InlineEdit {
    kind: InlineKind,
    /// 新建时的父目录；重命名时的原路径
    target: PathBuf,
    input: Entity<InputState>,
}

impl InlineEdit {
    /// 输入框所在的树行 id：新建时是占位行，重命名时是原路径
    fn row_id(&self) -> String {
        match self.kind {
            InlineKind::Rename => self.target.to_string_lossy().to_string(),
            _ => fs_ops::placeholder_id(&self.target),
        }
    }
}

/// 渲染树行时用到的内联输入快照
#[derive(Clone)]
struct InlineRow {
    id: SharedString,
    kind: InlineKind,
    input: Entity<InputState>,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum ViewMode {
    /// 所见即所得
    Edit,
    /// 只看源码
    Source,
    /// 只看演示
    Preview,
    /// 源码与演示对照
    Split,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum SidebarView {
    Explorer,
    Search,
    SourceControl,
}

/// 编辑器可选字体
#[derive(Clone, Copy, PartialEq, Eq)]
enum FontChoice {
    Hei,
    Song,
    WenKai,
    PuHuiTi,
}

const FONT_SIZES: [f32; 11] = [10., 12., 14., 16., 18., 20., 22., 24., 28., 32., 36.];

/// 编辑区正文宽度
#[derive(Clone, Copy, PartialEq, Eq)]
enum ColumnWidth {
    Standard,
    Wide,
    Narrow,
}

impl ColumnWidth {
    const ALL: [Self; 3] = [Self::Wide, Self::Standard, Self::Narrow];

    fn label(self) -> &'static str {
        match self {
            Self::Standard => "标准",
            Self::Wide => "宽",
            Self::Narrow => "窄",
        }
    }

    /// 正文容器的最大宽度
    fn max_width(self) -> f32 {
        match self {
            Self::Standard => 820.,
            Self::Wide => 1120.,
            Self::Narrow => 600.,
        }
    }
}

/// 分屏中线的拖拽载荷
struct SplitDrag;

/// 侧栏与编辑区之间分隔条的拖拽载荷
struct SidebarDrag;

/// 侧栏宽度的下限和上限，避免把目录树或编辑区挤没
const MIN_SIDEBAR: f32 = 180.;
const MAX_SIDEBAR: f32 = 560.;

/// 源代码管理里「改动 / 历史」分隔条的拖拽载荷
struct HistoryDrag;

/// 拖拽时跟随鼠标的占位元素（本体不可见，只为借用 GPUI 的拖拽管线）
struct SplitGhost;

impl Render for SplitGhost {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        div().w(px(0.)).h(px(0.))
    }
}

/// 分屏比例的下限和上限，避免任一侧被拖没
const MIN_SPLIT: f32 = 0.15;
const MAX_SPLIT: f32 = 0.85;

/// 历史提交面板的高度范围
const MIN_HISTORY: f32 = 72.;
const DEFAULT_HISTORY: f32 = 220.;

impl FontChoice {
    const ALL: [Self; 4] = [Self::Hei, Self::Song, Self::WenKai, Self::PuHuiTi];

    /// 工具栏上的单字按钮，完整名称放在 tooltip 里
    fn short(self) -> &'static str {
        match self {
            Self::Hei => "黑",
            Self::Song => "宋",
            Self::WenKai => "霞",
            Self::PuHuiTi => "阿",
        }
    }

    fn label(self) -> &'static str {
        match self {
            Self::Hei => "黑体",
            Self::Song => "宋体",
            Self::WenKai => "霞鹜文楷",
            Self::PuHuiTi => "阿里巴巴普惠体",
        }
    }
}

/// 编辑器配色
#[derive(Clone, Copy, PartialEq, Eq)]
enum ThemeChoice {
    Plain,
    Elegant,
    Night,
    Meadow,
}

struct Palette {
    background: u32,
    foreground: u32,
    muted: u32,
    muted_foreground: u32,
    border: u32,
    panel: u32,
    accent: u32,
}

/// 四种配色共用的强调色：靛蓝 #3b82f6。只调不透明度，色相始终一致。
/// 浅色底 30% 已是清晰色块；深色底要 45%，30% 会被底色吃掉。
const ACCENT_RGB: u32 = 0x3b82f6;
const SELECTION_ALPHA_LIGHT: u32 = 0x4d;
const SELECTION_ALPHA_DARK: u32 = 0x73;
/// 悬停态用同色更淡的一档，深色主题同样略加重
const HOVER_ALPHA_LIGHT: u32 = 0x2b;
const HOVER_ALPHA_DARK: u32 = 0x52;
/// ghost 按钮的悬停底在深色主题会被再乘 0.5，这里补一档更浓的
const GHOST_HOVER_ALPHA_DARK: u32 = 0x66;

/// 拼出不透明的十六进制色值 + 不透明度，得到 rgba(0xRRGGBBAA)
fn tint(alpha: u32) -> u32 {
    (ACCENT_RGB << 8) | alpha
}

impl ThemeChoice {
    const ALL: [Self; 4] = [Self::Plain, Self::Elegant, Self::Night, Self::Meadow];

    fn label(self) -> &'static str {
        match self {
            Self::Plain => "简白",
            Self::Elegant => "雅致",
            Self::Night => "黑夜",
            Self::Meadow => "绿茵",
        }
    }

    fn is_dark(self) -> bool {
        matches!(self, Self::Night)
    }

    fn palette(self) -> Palette {
        match self {
            Self::Plain => Palette {
                background: 0xffffff,
                foreground: 0x1f2328,
                muted: 0xf3f4f6,
                muted_foreground: 0x6e7781,
                border: 0xe4e6ea,
                panel: 0xf7f8f9,
                accent: 0xeceef1,
            },
            Self::Elegant => Palette {
                background: 0xfaf7f0,
                foreground: 0x33302a,
                muted: 0xf1ede3,
                muted_foreground: 0x7d7566,
                border: 0xe4ddcf,
                panel: 0xf5f1e7,
                accent: 0xebe4d4,
            },
            Self::Night => Palette {
                background: 0x1c1d1f,
                foreground: 0xe6e6e6,
                muted: 0x27292c,
                muted_foreground: 0x9aa0a6,
                border: 0x323437,
                panel: 0x212225,
                accent: 0x2f3236,
            },
            Self::Meadow => Palette {
                background: 0xf2f8ee,
                foreground: 0x27352b,
                muted: 0xe7f0e0,
                muted_foreground: 0x6d7d70,
                border: 0xd6e4ce,
                panel: 0xecf4e6,
                accent: 0xdfecd5,
            },
        }
    }
}

/// 打开中的文本文件
struct Doc {
    path: PathBuf,
    /// 源码编辑器：分屏视图使用，也是切到演示前的文本来源
    editor: Entity<EditorState>,
    /// 所见即所得编辑器：编辑视图使用
    wysiwyg: Entity<BlockEditorState>,
    wys_scroll: ScrollHandle,
    /// 演示视图的渲染状态。存成实体而不是每帧新建，查找时才能给它加高亮并滚动
    preview: Entity<TextViewState>,
    /// 最近一次同步给演示视图的 Markdown（已做硬换行处理）
    preview_src: String,
    /// 最近一次载入或保存时的规范 Markdown。所见即所得编辑器会把文档重新
    /// 序列化成自己的规范形式，用它当基线，打开文件才不会被误判成已修改
    baseline: String,
    view: ViewMode,
}

impl Doc {
    /// 当前视图下文档的真实文本
    fn text(&self, cx: &App) -> String {
        match self.view {
            ViewMode::Edit => self.wysiwyg.read(cx).source(),
            _ => self.editor.read(cx).value().to_string(),
        }
    }

    /// 与基线比较，忽略结尾空行（所见即所得往返会去掉尾部空行）
    fn is_dirty(&self, cx: &App) -> bool {
        self.text(cx).trim_end() != self.baseline.trim_end()
    }
}

/// 标签页：文本文件或差异视图
enum Tab {
    File(Doc),
    Diff {
        title: SharedString,
        state: Entity<DiffState>,
    },
}

impl Tab {
    fn path(&self) -> Option<&Path> {
        match self {
            Tab::File(doc) => Some(&doc.path),
            Tab::Diff { .. } => None,
        }
    }

    fn dirty(&self, cx: &App) -> bool {
        match self {
            Tab::File(doc) => doc.is_dirty(cx),
            Tab::Diff { .. } => false,
        }
    }

    /// 标签只显示文件名，目录层级由左侧资源管理器体现
    fn title(&self) -> SharedString {
        match self {
            Tab::File(doc) => doc
                .path
                .file_name()
                .map(|name| name.to_string_lossy().to_string())
                .unwrap_or_else(|| doc.path.to_string_lossy().to_string())
                .into(),
            Tab::Diff { title, .. } => title.clone(),
        }
    }
}

struct GitPanel {
    is_repo: bool,
    branch: String,
    changes: Vec<git::Change>,
    log: Vec<git::Commit>,
    branches: Vec<String>,
    input: Entity<InputState>,
    loading: bool,
}

/// 全局搜索面板
struct SearchPanel {
    input: Entity<InputState>,
    replace: Entity<InputState>,
    /// 最近一次发起搜索的关键词
    query: String,
    case_sensitive: bool,
    results: Vec<search::FileHits>,
    total: usize,
    truncated: bool,
    searching: bool,
    /// 每次发起搜索自增，后台结果返回时用来丢弃过期的那次
    generation: u64,
}

/// 当前文档的查找替换栏
struct FindPanel {
    open: bool,
    /// 是否展开「替换为」那一行
    show_replace: bool,
    input: Entity<InputState>,
    replace: Entity<InputState>,
    case_sensitive: bool,
    /// 当前文档里的命中，按字节区间记
    matches: Vec<(usize, usize)>,
    current: usize,
}

pub struct NotesApp {
    root: PathBuf,
    tree_state: Entity<TreeState>,
    expanded: HashSet<String>,
    tabs: Vec<Tab>,
    active: Option<usize>,
    /// 目录树里的内联输入（新建 / 重命名），None 表示没有
    inline: Option<InlineEdit>,
    status: SharedString,
    sidebar: SidebarView,
    show_branches: bool,
    git: GitPanel,
    search: SearchPanel,
    find: FindPanel,
    font: FontChoice,
    font_size: f32,
    theme_choice: ThemeChoice,
    column_width: ColumnWidth,
    /// 分屏时左栏（源码）所占比例
    split_ratio: f32,
    /// 侧栏宽度，可拖拽调整
    sidebar_width: f32,
    /// 源代码管理里历史提交面板的高度
    history_height: f32,
    /// 切换工作区后待打开的文档：创建编辑器需要 Window，留到渲染时打开
    pending_open: Option<PathBuf>,
    wenkai_family: SharedString,
    puhuiti_family: SharedString,
    /// 目录树与标签栏使用的彩色文件图标
    icons: FileIcons,
}

impl NotesApp {
    pub fn new(root: PathBuf, window: &mut Window, cx: &mut Context<Self>) -> Self {
        let (wenkai_family, puhuiti_family) = load_reading_fonts(cx);
        let icons = FileIcons::new(cx);
        let session = fs_ops::load_session();
        let expanded = HashSet::new();
        let items = fs_ops::build_items(&root, &expanded);
        let tree_state = cx.new(|cx| TreeState::new(cx).items(items));

        cx.subscribe(&tree_state, |this, _, event: &TreeEvent, _| match event {
            TreeEvent::Expanded(id) => {
                this.expanded.insert(id.to_string());
            }
            TreeEvent::Collapsed(id) => {
                this.expanded.remove(id.as_ref());
            }
        })
        .detach();
        cx.observe(&tree_state, |_, _, cx| cx.notify()).detach();

        let input = cx.new(|cx| InputState::new(window, cx).placeholder("提交说明"));
        let search_input = cx.new(|cx| InputState::new(window, cx).placeholder("搜索"));
        let replace_input = cx.new(|cx| InputState::new(window, cx).placeholder("替换为"));
        let find_input = cx.new(|cx| InputState::new(window, cx).placeholder("查找"));
        let find_replace_input = cx.new(|cx| InputState::new(window, cx).placeholder("替换为"));

        let mut this = Self {
            root,
            tree_state,
            expanded,
            tabs: Vec::new(),
            active: None,
            inline: None,
            status: "".into(),
            sidebar: SidebarView::Explorer,
            show_branches: false,
            git: GitPanel {
                is_repo: false,
                branch: String::new(),
                changes: Vec::new(),
                log: Vec::new(),
                branches: Vec::new(),
                input,
                loading: false,
            },
            search: SearchPanel {
                input: search_input.clone(),
                replace: replace_input,
                query: String::new(),
                case_sensitive: false,
                results: Vec::new(),
                total: 0,
                truncated: false,
                searching: false,
                generation: 0,
            },
            find: FindPanel {
                open: false,
                show_replace: false,
                input: find_input.clone(),
                replace: find_replace_input,
                case_sensitive: false,
                matches: Vec::new(),
                current: 0,
            },
            font: FontChoice::Hei,
            font_size: 18.,
            theme_choice: ThemeChoice::Elegant,
            column_width: ColumnWidth::Standard,
            split_ratio: 0.5,
            sidebar_width: 272.,
            history_height: DEFAULT_HISTORY,
            pending_open: None,
            wenkai_family,
            puhuiti_family,
            icons,
        };
        this.apply_theme(ThemeChoice::Elegant, cx);
        this.refresh_git(cx);

        // 边打字边搜索
        cx.subscribe(&search_input, |this, _, event: &InputEvent, cx| {
            if matches!(event, InputEvent::Change) {
                this.start_search(cx);
            }
        })
        .detach();

        // 查找栏里输入时，实时更新当前文档的命中；回车跳到下一个
        cx.subscribe(&find_input, |this, _, event: &InputEvent, cx| match event {
            InputEvent::Change => this.refresh_find(cx),
            InputEvent::PressEnter { shift, .. } => this.find_step(!*shift, cx),
            _ => {}
        })
        .detach();

        // 恢复上次打开的文档，已经删除的文件直接跳过
        let mut restored = 0;
        if let Some(session) = &session {
            for path in &session.files {
                if path.is_file() {
                    this.open_file(path.clone(), window, cx);
                    restored += 1;
                }
            }
        }
        if restored > 0 {
            if let Some(ix) = session.as_ref().and_then(|s| s.active) {
                if ix < this.tabs.len() {
                    this.active = Some(ix);
                    this.focus_active(window, cx);
                }
            }
        } else {
            // 首次运行（工作区为空）时生成欢迎文档，并把它作为启动页打开
            let welcome = this.root.join(fs_ops::WELCOME_NAME);
            if !welcome.exists() && fs_ops::is_empty_dir(&this.root) {
                let _ = fs_ops::write_welcome(&this.root);
            }
            if welcome.exists() {
                this.open_file(welcome, window, cx);
            }
        }
        this.persist_session();
        this
    }

    fn set_status(&mut self, message: impl Into<SharedString>, cx: &mut Context<Self>) {
        self.status = message.into();
        cx.notify();
    }

    /// 把工作区与当前打开的文档写回会话文件
    fn persist_session(&self) {
        let files: Vec<PathBuf> = self
            .tabs
            .iter()
            .filter_map(|tab| tab.path().map(Path::to_path_buf))
            .collect();
        fs_ops::save_session(&self.root, &files, self.active);
    }

    // ---------- 排版 ----------

    fn font_family(&self) -> SharedString {
        match self.font {
            FontChoice::Hei => "SimHei".into(),
            FontChoice::Song => "SimSun".into(),
            FontChoice::WenKai => self.wenkai_family.clone(),
            FontChoice::PuHuiTi => self.puhuiti_family.clone(),
        }
    }

    fn apply_theme(&mut self, choice: ThemeChoice, cx: &mut Context<Self>) {
        self.theme_choice = choice;
        let mode = if choice.is_dark() {
            ThemeMode::Dark
        } else {
            ThemeMode::Light
        };
        // 先切模式（会重新载入内置配色），再覆盖成本应用的配色
        Theme::change(mode, None, cx);
        let palette = choice.palette();
        // 深色底需要更浓的一档，否则强调色会被背景吃掉
        let dark = choice.is_dark();
        let selection = tint(if dark { SELECTION_ALPHA_DARK } else { SELECTION_ALPHA_LIGHT });
        let hover = tint(if dark { HOVER_ALPHA_DARK } else { HOVER_ALPHA_LIGHT });
        let ghost_hover = tint(if dark { GHOST_HOVER_ALPHA_DARK } else { HOVER_ALPHA_LIGHT });
        Theme::update(cx, |theme| {
            let colors = &mut theme.colors;
            colors.background = rgb(palette.background).into();
            colors.foreground = rgb(palette.foreground).into();
            colors.muted = rgb(palette.muted).into();
            colors.muted_foreground = rgb(palette.muted_foreground).into();
            colors.border = rgb(palette.border).into();
            colors.input = rgb(palette.background).into();
            // accent 是 ghost 按钮的悬停底，换成强调色的淡色，深色主题才看得出按钮边界
            colors.accent = rgba(ghost_hover).into();
            colors.accent_foreground = rgb(palette.foreground).into();
            colors.selection = rgba(selection).into();
            colors.caret = rgb(palette.foreground).into();
            colors.sidebar = rgb(palette.panel).into();
            colors.sidebar_foreground = rgb(palette.foreground).into();
            colors.sidebar_border = rgb(palette.border).into();
            colors.sidebar_accent = rgb(palette.accent).into();
            colors.sidebar_accent_foreground = rgb(palette.foreground).into();
            colors.tab_bar = rgb(palette.panel).into();
            colors.tab = rgb(palette.panel).into();
            colors.tab_active = rgb(palette.background).into();
            colors.tab_foreground = rgb(palette.muted_foreground).into();
            colors.tab_active_foreground = rgb(palette.foreground).into();
            colors.title_bar = rgb(palette.panel).into();
            colors.title_bar_border = rgb(palette.border).into();
            colors.status_bar = rgb(palette.panel).into();
            colors.status_bar_border = rgb(palette.border).into();
            colors.list = rgb(palette.background).into();
            colors.list_hover = rgba(hover).into();
            colors.list_active = rgba(selection).into();
            colors.list_even = rgb(palette.muted).into();
            colors.popover = rgb(palette.background).into();
            colors.popover_foreground = rgb(palette.foreground).into();
            colors.scrollbar_thumb = rgb(palette.border).into();
            colors.button = rgb(palette.muted).into();
            colors.button_hover = rgba(hover).into();
            colors.button_active = rgba(selection).into();
            colors.button_foreground = rgb(palette.foreground).into();
            // ghost 按钮的 selected 态取 secondary_active，工具栏的字体/字号/背景/宽度按钮全靠它
            colors.secondary_active = rgba(selection).into();
            // 代码块语法高亮：深色用 Dracula，浅色用官方的浅色变体 Alucard
            theme.highlight_theme = crate::syntax::highlight_theme(dark);
        });
        cx.notify();
    }

    // ---------- 标签页 ----------

    fn active_doc(&self) -> Option<&Doc> {
        match self.active.and_then(|ix| self.tabs.get(ix)) {
            Some(Tab::File(doc)) => Some(doc),
            _ => None,
        }
    }

    fn make_editor(text: &str, window: &mut Window, cx: &mut Context<Self>) -> Entity<EditorState> {
        let editor = cx
            .new(|cx| EditorState::new(window, cx).line_number(true).searchable(false));
        editor.update(cx, |state, cx| state.set_value(text, window, cx));

        cx.subscribe(&editor, |_: &mut NotesApp, _, event: &InputEvent, cx| {
            // 未保存状态在渲染时按文本比较得出，这里只负责触发重绘
            if matches!(event, InputEvent::Change) {
                cx.notify();
            }
        })
        .detach();

        editor
    }

    fn make_block_editor(
        text: &str,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> (Entity<BlockEditorState>, ScrollHandle) {
        let scroll = ScrollHandle::new();
        let editor = cx.new(|cx| {
            let mut state = BlockEditorState::new(window, cx).with_source(text);
            // 让光标移动时自动滚动到可见区域
            state.set_scroll_handle(scroll.clone());
            state
        });

        cx.subscribe(&editor, |_: &mut NotesApp, _, _: &BlockEditorEvent, cx| {
            // 脏状态在渲染时按文本比较得出，这里只负责触发重绘
            cx.notify();
        })
        .detach();

        (editor, scroll)
    }

    fn open_file(&mut self, path: PathBuf, window: &mut Window, cx: &mut Context<Self>) {
        if path.is_dir() {
            return;
        }
        if let Some(ix) = self
            .tabs
            .iter()
            .position(|t| t.path() == Some(path.as_path()))
        {
            self.activate(ix, window, cx);
            return;
        }

        match std::fs::read_to_string(&path) {
            Ok(text) => {
                let editor = Self::make_editor(&text, window, cx);
                let (wysiwyg, wys_scroll) = Self::make_block_editor(&text, window, cx);
                let preview_src = preview_source(&text);
                let preview = cx.new(|cx| TextViewState::markdown(&preview_src, cx));
                // 所见即所得重新序列化后会得到规范形式，与原文格式未必一致，
                // 以它当基线，打开文件不会立刻被当成改动
                let baseline = wysiwyg.read(cx).source();
                self.tabs.push(Tab::File(Doc {
                    path: path.clone(),
                    editor,
                    wysiwyg,
                    wys_scroll,
                    preview,
                    preview_src,
                    baseline,
                    view: ViewMode::Edit,
                }));
                self.active = Some(self.tabs.len() - 1);
                self.reveal_parent(&path);
                self.rescan(cx);
                self.focus_active(window, cx);
                self.sync_selection(cx);
                self.persist_session();
                cx.notify();
            }
            Err(err) => self.set_status(format!("打开失败：{err}"), cx),
        }
    }

    fn focus_active(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(ix) = self.active else {
            return;
        };
        let Some(Tab::File(doc)) = self.tabs.get_mut(ix) else {
            return;
        };
        match doc.view {
            ViewMode::Edit => doc.wysiwyg.update(cx, |state, cx| state.focus(window, cx)),
            _ => doc.editor.update(cx, |state, cx| state.focus(window, cx)),
        }
    }

    /// 切换视图，并在源码编辑器与所见即所得编辑器之间同步文本
    fn switch_view(&mut self, view: ViewMode, window: &mut Window, cx: &mut Context<Self>) {
        let Some(ix) = self.active else {
            return;
        };
        let Some(Tab::File(doc)) = self.tabs.get_mut(ix) else {
            return;
        };
        if doc.view == view {
            return;
        }

        if doc.view == ViewMode::Edit {
            let text = doc.wysiwyg.read(cx).source();
            doc.editor
                .update(cx, |state, cx| state.set_value(text, window, cx));
        }
        if view == ViewMode::Edit {
            let text = doc.editor.read(cx).value().to_string();
            doc.wysiwyg
                .update(cx, |state, cx| state.set_source(&text, cx).ok());
            // 源码视图没有实际改动时，把基线换成重新规范化后的文本，
            // 免得只是切了一下视图就被标记成已修改
            if text.trim_end() == doc.baseline.trim_end() {
                doc.baseline = doc.wysiwyg.read(cx).source();
            }
        }

        doc.view = view;
        match view {
            ViewMode::Edit => doc.wysiwyg.update(cx, |state, cx| state.focus(window, cx)),
            _ => doc.editor.update(cx, |state, cx| state.focus(window, cx)),
        }
        // 查找栏开着时，切换视图后重新落一次高亮和位置
        if self.find.open {
            self.refresh_find(cx);
        }
        cx.notify();
    }

    fn activate(&mut self, ix: usize, window: &mut Window, cx: &mut Context<Self>) {
        if ix >= self.tabs.len() {
            return;
        }
        if self.active == Some(ix) {
            self.focus_active(window, cx);
            return;
        }
        self.save_active(cx);
        self.active = Some(ix);
        self.focus_active(window, cx);
        self.sync_selection(cx);
        // 换了文档，查找栏里的关键词要按新文档重新算
        if self.find.open {
            self.refresh_find(cx);
        }
        self.persist_session();
        cx.notify();
    }

    fn save_doc(&mut self, ix: usize, cx: &mut Context<Self>) -> bool {
        let Some(Tab::File(doc)) = self.tabs.get_mut(ix) else {
            return false;
        };
        // 没有真正编辑过就不写盘，避免把文件整体重排成另一种写法
        if !doc.is_dirty(cx) {
            return true;
        }
        let text = doc.text(cx);

        match std::fs::write(&doc.path, &text) {
            Ok(_) => {
                doc.baseline = text;
                true
            }
            Err(err) => {
                let message = format!("保存失败：{err}");
                self.set_status(message, cx);
                false
            }
        }
    }

    fn save_active(&mut self, cx: &mut Context<Self>) -> bool {
        match self.active {
            Some(ix) => self.save_doc(ix, cx),
            None => false,
        }
    }

    fn on_save(&mut self, _: &Save, window: &mut Window, cx: &mut Context<Self>) {
        if self.active.is_none() {
            self.set_status("还没有打开笔记", cx);
            return;
        }
        if self.save_active(cx) {
            self.set_status("已保存", cx);
        }
        self.refresh_git(cx);
        let _ = window;
    }

    fn request_close(&mut self, ix: usize, window: &mut Window, cx: &mut Context<Self>) {
        if ix >= self.tabs.len() {
            return;
        }
        if !self.tabs[ix].dirty(cx) {
            self.close_tab_now(ix, window, cx);
            return;
        }

        let view = cx.entity();
        window.open_dialog(cx, move |dialog, _, _| {
            let save_view = view.clone();
            let discard_view = view.clone();
            dialog
                .title("未保存的修改")
                .child("关闭这个标签页前要保存修改吗？")
                .footer(
                    DialogFooter::new()
                        .child(
                            Button::new("cancel")
                                .label("取消")
                                .on_click(|_, window, cx| window.close_dialog(cx)),
                        )
                        .child(Button::new("discard").label("不保存").on_click(
                            move |_, window, cx| {
                                window.close_dialog(cx);
                                discard_view.update(cx, |this, cx| {
                                    this.close_tab_now(ix, window, cx)
                                });
                            },
                        ))
                        .child(Button::new("save").primary().label("保存并关闭").on_click(
                            move |_, window, cx| {
                                window.close_dialog(cx);
                                save_view.update(cx, |this, cx| {
                                    this.save_doc(ix, cx);
                                    this.close_tab_now(ix, window, cx);
                                });
                            },
                        )),
                )
        });
    }

    fn close_tab_now(&mut self, ix: usize, window: &mut Window, cx: &mut Context<Self>) {
        if ix >= self.tabs.len() {
            return;
        }
        self.tabs.remove(ix);

        if self.tabs.is_empty() {
            self.active = None;
        } else {
            let next = match self.active {
                Some(active) if active > ix => active - 1,
                Some(active) if active == ix => ix.min(self.tabs.len() - 1),
                Some(active) => active,
                None => 0,
            };
            self.active = Some(next);
            self.focus_active(window, cx);
        }
        self.sync_selection(cx);
        self.persist_session();
        cx.notify();
    }

    fn on_close_tab(&mut self, _: &CloseTab, window: &mut Window, cx: &mut Context<Self>) {
        if let Some(ix) = self.active {
            self.request_close(ix, window, cx);
        }
    }

    fn on_next_tab(&mut self, _: &NextTab, window: &mut Window, cx: &mut Context<Self>) {
        if self.tabs.is_empty() {
            return;
        }
        let next = match self.active {
            Some(active) => (active + 1) % self.tabs.len(),
            None => 0,
        };
        self.activate(next, window, cx);
    }

    fn on_prev_tab(&mut self, _: &PrevTab, window: &mut Window, cx: &mut Context<Self>) {
        if self.tabs.is_empty() {
            return;
        }
        let prev = match self.active {
            Some(active) => (active + self.tabs.len() - 1) % self.tabs.len(),
            None => 0,
        };
        self.activate(prev, window, cx);
    }

    fn on_toggle_preview(&mut self, _: &TogglePreview, window: &mut Window, cx: &mut Context<Self>) {
        let Some(ix) = self.active else {
            return;
        };
        if let Some(Tab::File(doc)) = self.tabs.get(ix) {
            let next = match doc.view {
                ViewMode::Edit => ViewMode::Source,
                ViewMode::Source => ViewMode::Preview,
                ViewMode::Preview => ViewMode::Split,
                ViewMode::Split => ViewMode::Edit,
            };
            self.switch_view(next, window, cx);
        }
    }

    // ---------- 目录树 ----------

    fn reveal_parent(&mut self, path: &Path) {
        let mut dir = path.parent();
        while let Some(d) = dir {
            if d == self.root.as_path() {
                break;
            }
            self.expanded.insert(d.to_string_lossy().to_string());
            dir = d.parent();
        }
    }

    fn rescan(&mut self, cx: &mut Context<Self>) {
        if let Some(path) = self.active_doc().map(|d| d.path.clone()) {
            self.reveal_parent(&path);
        }
        // set_items 会清空选中项，先记下，重建后选回去
        let selected = self
            .tree_state
            .read(cx)
            .selected_item()
            .map(|item| item.id.clone());
        let root = self.root.clone();
        let expanded = self.expanded.clone();
        let mut items = fs_ops::build_items(&root, &expanded);
        // 内联新建时给目标目录插一行占位，输入框就画在那一行
        if let Some(edit) = self.inline.as_ref() {
            if edit.kind != InlineKind::Rename {
                fs_ops::insert_placeholder(&mut items, &root, &edit.target, "新名称");
            }
        }
        self.tree_state.update(cx, |state, cx| {
            state.set_items(items, cx);
            if let Some(id) = selected.as_ref() {
                let ix = state.index_of(id);
                state.set_selected_index(ix, cx);
            }
        });
    }

    fn sync_selection(&mut self, cx: &mut Context<Self>) {
        let target = self
            .active_doc()
            .map(|doc| SharedString::from(doc.path.to_string_lossy().to_string()));
        self.tree_state.update(cx, |state, cx| {
            let ix = target.as_ref().and_then(|id| state.index_of(id));
            state.set_selected_index(ix, cx);
        });
    }

    fn selected_path(&self, cx: &App) -> Option<PathBuf> {
        self.tree_state
            .read(cx)
            .selected_item()
            .map(|item| PathBuf::from(item.id.to_string()))
    }

    // ---------- 新建 / 重命名（目录树内联输入） ----------

    /// 在目录树里就地输入名字。新建时 `target` 是父目录，重命名时是要改的路径
    fn start_inline(
        &mut self,
        kind: InlineKind,
        target: PathBuf,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let initial = match kind {
            InlineKind::NewFile => fs_ops::next_untitled(&target),
            InlineKind::NewFolder => fs_ops::next_folder_name(&target),
            InlineKind::Rename => target
                .file_name()
                .map(|n| n.to_string_lossy().to_string())
                .unwrap_or_default(),
        };

        // 新建的输入框画在目标目录里，目录要展开才看得见
        if kind != InlineKind::Rename {
            self.reveal_parent(&target.join("x"));
        }

        let input = cx.new(|cx| InputState::new(window, cx).default_value(initial));
        // 点到别处就算放弃；重名等提示会重新聚焦，不会被这里当成失焦
        cx.subscribe(&input, |this, input, event: &InputEvent, cx| {
            if matches!(event, InputEvent::Blur)
                && this
                    .inline
                    .as_ref()
                    .is_some_and(|edit| edit.input == input)
            {
                this.cancel_inline(cx);
            }
        })
        .detach();
        self.inline = Some(InlineEdit {
            kind,
            target,
            input: input.clone(),
        });
        self.rescan(cx);

        let handle = input.read(cx).focus_handle(cx).clone();
        handle.focus(window, cx);
        // 全选默认名，直接输入即可替换
        input.update(cx, |state, cx| state.select_all(window, cx));
        self.set_status("输入名称，回车确认，Esc 取消", cx);
    }

    /// 内联输入的回车：落盘并收起输入框；名字不合法或有冲突时留在原地提示
    fn commit_inline(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(edit) = self.inline.take() else {
            return;
        };
        let name = edit.input.read(cx).value().trim().to_string();
        if !fs_ops::is_valid_name(&name) {
            self.keep_inline(
                edit,
                "名称不能为空，且不能包含 \\ / : * ? \" < > |",
                window,
                cx,
            );
            return;
        }

        match edit.kind {
            InlineKind::NewFile => {
                let mut file_name = name;
                if Path::new(&file_name).extension().is_none() {
                    file_name.push_str(".md");
                }
                let path = edit.target.join(&file_name);
                if path.exists() {
                    self.keep_inline(edit, format!("“{file_name}”已存在"), window, cx);
                    return;
                }
                if let Err(err) = std::fs::write(&path, "") {
                    self.keep_inline(edit, format!("创建失败：{err}"), window, cx);
                    return;
                }
                self.set_status(format!("已创建 {file_name}"), cx);
                self.rescan(cx);
                // 打开文件要 Window，留到下一次渲染
                self.pending_open = Some(path);
            }
            InlineKind::NewFolder => {
                let path = edit.target.join(&name);
                if path.exists() {
                    self.keep_inline(edit, format!("“{name}”已存在"), window, cx);
                    return;
                }
                if let Err(err) = std::fs::create_dir_all(&path) {
                    self.keep_inline(edit, format!("创建失败：{err}"), window, cx);
                    return;
                }
                self.set_status(format!("已创建文件夹 {name}"), cx);
                self.rescan(cx);
                // 选中刚建好的目录，便于继续在里面新建内容
                let id = SharedString::from(path.to_string_lossy().to_string());
                self.tree_state.update(cx, |state, cx| {
                    let ix = state.index_of(&id);
                    state.set_selected_index(ix, cx);
                });
            }
            InlineKind::Rename => {
                let path = edit.target.clone();
                let mut new_name = name;
                // 重命名文件时若没写扩展名，保留原扩展名
                if path.is_file() && Path::new(&new_name).extension().is_none() {
                    if let Some(ext) = path.extension() {
                        new_name.push('.');
                        new_name.push_str(&ext.to_string_lossy());
                    }
                }
                let new_path = path.with_file_name(&new_name);
                if new_path != path {
                    if new_path.exists() {
                        self.keep_inline(edit, format!("“{new_name}”已存在"), window, cx);
                        return;
                    }
                    if let Err(err) = std::fs::rename(&path, &new_path) {
                        self.keep_inline(edit, format!("重命名失败：{err}"), window, cx);
                        return;
                    }
                    self.remap_paths(&path, &new_path);
                    self.set_status("已重命名", cx);
                }
                self.rescan(cx);
            }
        }

        self.refresh_git(cx);
        cx.notify();
    }

    /// 提交失败：把输入框放回去，并重新选中原来的名字
    fn keep_inline(
        &mut self,
        edit: InlineEdit,
        message: impl Into<SharedString>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let input = edit.input.clone();
        self.inline = Some(edit);
        self.set_status(message, cx);
        let handle = input.read(cx).focus_handle(cx).clone();
        handle.focus(window, cx);
        input.update(cx, |state, cx| state.select_all(window, cx));
    }

    /// Esc 或点到别处：放弃输入，占位行随之消失
    fn cancel_inline(&mut self, cx: &mut Context<Self>) {
        if self.inline.take().is_some() {
            self.rescan(cx);
            cx.notify();
        }
    }

    /// 重命名目录后，同步展开状态和已打开标签的路径
    fn remap_paths(&mut self, from: &Path, to: &Path) {
        self.expanded = self
            .expanded
            .iter()
            .map(|id| {
                let p = Path::new(id);
                if p == from {
                    to.to_string_lossy().to_string()
                } else if let Ok(rest) = p.strip_prefix(from) {
                    to.join(rest).to_string_lossy().to_string()
                } else {
                    id.clone()
                }
            })
            .collect();

        for tab in &mut self.tabs {
            if let Tab::File(doc) = tab {
                if doc.path == from {
                    doc.path = to.to_path_buf();
                } else if let Ok(rest) = doc.path.strip_prefix(from) {
                    doc.path = to.join(rest);
                }
            }
        }

        self.persist_session();
    }

    // ---------- 删除 ----------

    fn confirm_delete(&mut self, path: PathBuf, window: &mut Window, cx: &mut Context<Self>) {
        let view = cx.entity();
        let name = path
            .file_name()
            .map(|n| n.to_string_lossy().to_string())
            .unwrap_or_default();
        let kind = if path.is_dir() { "文件夹" } else { "笔记" };
        let title: SharedString = format!("删除{kind}").into();

        window.open_dialog(cx, move |dialog, _, _| {
            let view = view.clone();
            let path = path.clone();
            dialog
                .title(title.clone())
                .child(SharedString::from(format!(
                    "确定要删除“{name}”吗？此操作不可恢复。"
                )))
                .footer(
                    DialogFooter::new()
                        .child(
                            Button::new("cancel")
                                .label("取消")
                                .on_click(|_, window, cx| window.close_dialog(cx)),
                        )
                        .child(
                            DialogAction::new()
                                .child(Button::new("ok").danger().label("删除")),
                        ),
                )
                .on_ok(move |_, window, cx| {
                    let view = view.clone();
                    let path = path.clone();
                    view.update(cx, |this, cx| this.do_delete(path, window, cx));
                    true
                })
        });
    }

    fn do_delete(&mut self, path: PathBuf, window: &mut Window, cx: &mut Context<Self>) {
        let result = if path.is_dir() {
            std::fs::remove_dir_all(&path)
        } else {
            std::fs::remove_file(&path)
        };

        match result {
            Ok(_) => {
                // 关闭被删文件（或已删目录下文件）的标签
                let hits: Vec<usize> = self
                    .tabs
                    .iter()
                    .enumerate()
                    .filter(|(_, tab)| {
                        tab.path()
                            .is_some_and(|p| p == path.as_path() || p.starts_with(&path))
                    })
                    .map(|(ix, _)| ix)
                    .collect();
                for ix in hits.into_iter().rev() {
                    self.tabs.remove(ix);
                }
                if self.tabs.is_empty() {
                    self.active = None;
                } else {
                    let last = self.tabs.len() - 1;
                    self.active = Some(self.active.unwrap_or(0).min(last));
                    self.focus_active(window, cx);
                }
                self.rescan(cx);
                self.sync_selection(cx);
                self.persist_session();
                self.set_status("已删除", cx);
                self.refresh_git(cx);
            }
            Err(err) => self.set_status(format!("删除失败：{err}"), cx),
        }
    }

    fn on_rename_selected(&mut self, _: &RenameSelected, window: &mut Window, cx: &mut Context<Self>) {
        match self.selected_path(cx) {
            Some(path) => self.start_inline(InlineKind::Rename, path, window, cx),
            None => self.set_status("先在左侧选中一个文件或文件夹", cx),
        }
    }

    fn on_delete_selected(&mut self, _: &DeleteSelected, window: &mut Window, cx: &mut Context<Self>) {
        match self.selected_path(cx) {
            Some(path) => self.confirm_delete(path, window, cx),
            None => self.set_status("先在左侧选中一个文件或文件夹", cx),
        }
    }

    fn on_refresh(&mut self, _: &Refresh, _: &mut Window, cx: &mut Context<Self>) {
        self.rescan(cx);
        self.refresh_git(cx);
        self.start_search(cx);
        self.set_status("已刷新", cx);
    }

    // ---------- 全局搜索 ----------

    fn on_focus_search(&mut self, _: &FocusSearch, window: &mut Window, cx: &mut Context<Self>) {
        self.show_search(window, cx);
    }

    /// 切到搜索面板，并把光标放进搜索框
    fn show_search(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.sidebar = SidebarView::Search;
        self.search
            .input
            .update(cx, |state, cx| state.focus(window, cx));
        cx.notify();
    }

    /// 按输入框里的关键词搜索整个工作区
    fn start_search(&mut self, cx: &mut Context<Self>) {
        let query = self.search.input.read(cx).value().to_string();
        self.search.query = query.clone();
        self.search.generation += 1;
        let generation = self.search.generation;

        if query.trim().is_empty() {
            self.search.results.clear();
            self.search.total = 0;
            self.search.truncated = false;
            self.search.searching = false;
            cx.notify();
            return;
        }

        let root = self.root.clone();
        let case_sensitive = self.search.case_sensitive;
        self.search.searching = true;
        cx.notify();

        cx.spawn(async move |this, cx| {
            let outcome = cx
                .background_executor()
                .spawn(async move { search::search(&root, &query, case_sensitive) })
                .await;

            this.update(cx, |this, cx| {
                // 打字快时会有多次搜索同时在跑，只认最后一次的结果
                if this.search.generation != generation {
                    return;
                }
                this.search.results = outcome.files;
                this.search.total = outcome.total;
                this.search.truncated = outcome.truncated;
                this.search.searching = false;
                cx.notify();
            })
            .ok();
        })
        .detach();
    }

    /// 打开命中的文件并定位到那一行，源码视图才看得见位置
    fn jump_to_hit(
        &mut self,
        path: PathBuf,
        line: usize,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.open_file(path.clone(), window, cx);
        self.switch_view(ViewMode::Source, window, cx);

        let Some(ix) = self.active else {
            return;
        };
        let Some(Tab::File(doc)) = self.tabs.get_mut(ix) else {
            return;
        };
        if doc.path != path {
            return;
        }
        let position = Position::new(line.saturating_sub(1) as u32, 0);
        doc.editor
            .update(cx, |state, cx| state.set_cursor_position(position, window, cx));
    }

    /// 替换确认框：先数一下会改多少处，再问一次
    fn confirm_replace(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let query = self.search.input.read(cx).value().to_string();
        if query.trim().is_empty() {
            self.set_status("先输入要查找的内容", cx);
            return;
        }
        let replacement = self.search.replace.read(cx).value().to_string();
        let mut paths = Vec::new();
        search::collect_files(&self.root, &mut paths);
        let case_sensitive = self.search.case_sensitive;
        let mut total = 0;
        let mut files = 0;
        for path in paths {
            let Ok(text) = std::fs::read_to_string(&path) else {
                continue;
            };
            let (_, count) = search::replace(&text, &query, &replacement, case_sensitive);
            if count > 0 {
                total += count;
                files += 1;
            }
        }
        if total == 0 {
            self.set_status("没有找到可替换的内容", cx);
            return;
        }

        let view = cx.entity();
        window.open_dialog(cx, move |dialog, _, _| {
            let view = view.clone();
            let query = query.clone();
            let replacement = replacement.clone();
            dialog
                .title("全部替换")
                .child(SharedString::from(format!(
                    "在工作区 {files} 个文件里替换 {total} 处“{query}”为“{replacement}”？"
                )))
                .footer(
                    DialogFooter::new()
                        .child(
                            Button::new("cancel")
                                .label("取消")
                                .on_click(|_, window, cx| window.close_dialog(cx)),
                        )
                        .child(DialogAction::new().child(Button::new("ok").label("替换"))),
                )
                .on_ok(move |_, window, cx| {
                    let view = view.clone();
                    let query = query.clone();
                    let replacement = replacement.clone();
                    view.update(cx, |this, cx| {
                        this.do_replace_all(&query, &replacement, window, cx)
                    });
                    true
                })
        });
    }

    fn do_replace_all(
        &mut self,
        query: &str,
        replacement: &str,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let case_sensitive = self.search.case_sensitive;
        let mut paths = Vec::new();
        search::collect_files(&self.root, &mut paths);

        let mut total = 0;
        let mut changed = 0;
        let mut failed = 0;

        for path in paths {
            // 已经打开的标签改内存里的文本并直接保存，免得磁盘与编辑器各写一份
            if let Some(ix) = self
                .tabs
                .iter()
                .position(|tab| tab.path() == Some(path.as_path()))
            {
                let Some(Tab::File(doc)) = self.tabs.get(ix) else {
                    continue;
                };
                let text = doc.text(cx);
                let (new_text, count) = search::replace(&text, query, replacement, case_sensitive);
                if count == 0 {
                    continue;
                }
                let Some(Tab::File(doc)) = self.tabs.get_mut(ix) else {
                    continue;
                };
                doc.editor
                    .update(cx, |state, cx| state.set_value(new_text.clone(), window, cx));
                doc.wysiwyg
                    .update(cx, |state, cx| state.set_source(&new_text, cx).ok());
                if self.save_doc(ix, cx) {
                    total += count;
                    changed += 1;
                } else {
                    failed += 1;
                }
                continue;
            }

            let Ok(text) = std::fs::read_to_string(&path) else {
                continue;
            };
            let (new_text, count) = search::replace(&text, query, replacement, case_sensitive);
            if count == 0 {
                continue;
            }
            match std::fs::write(&path, new_text) {
                Ok(_) => {
                    total += count;
                    changed += 1;
                }
                Err(_) => failed += 1,
            }
        }

        let message = if total == 0 {
            "没有找到可替换的内容".to_string()
        } else if failed > 0 {
            format!("已在 {changed} 个文件替换 {total} 处，{failed} 个文件写入失败")
        } else {
            format!("已在 {changed} 个文件替换 {total} 处")
        };
        self.set_status(message, cx);
        self.rescan(cx);
        self.refresh_git(cx);
        self.start_search(cx);
    }

    // ---------- 文档内查找替换 ----------

    fn on_focus_find(&mut self, _: &FocusFind, window: &mut Window, cx: &mut Context<Self>) {
        // 再按一次就把查找栏收起来（替换行展开时也一样）
        if self.find.open {
            self.close_find(window, cx);
            return;
        }
        self.open_find(false, window, cx);
    }

    fn on_open_replace(&mut self, _: &OpenReplace, window: &mut Window, cx: &mut Context<Self>) {
        // 替换行已经展开时，再按一次同样收起
        if self.find.open && self.find.show_replace {
            self.close_find(window, cx);
            return;
        }
        self.open_find(true, window, cx);
    }

    /// 打开查找栏；`show_replace` 为真时一并展开替换那一行
    fn open_find(&mut self, show_replace: bool, window: &mut Window, cx: &mut Context<Self>) {
        if self.active_doc().is_none() {
            self.set_status("先打开一篇笔记", cx);
            return;
        }
        self.find.open = true;
        self.find.show_replace = show_replace;
        self.find
            .input
            .update(cx, |state, cx| state.focus(window, cx));
        self.refresh_find(cx);
        cx.notify();
    }

    fn close_find(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.find.open = false;
        self.find.matches.clear();
        self.find.current = 0;
        self.clear_preview_highlights(cx);
        self.focus_active(window, cx);
        cx.notify();
    }

    /// 按查找框里的关键词重新计算当前文档的命中，并把视图滚到当前命中
    fn refresh_find(&mut self, cx: &mut Context<Self>) {
        let query = self.find.input.read(cx).value().to_string();
        let case_sensitive = self.find.case_sensitive;

        let matches = match self.active_doc() {
            Some(doc) => search::occurrences(&doc.text(cx), &query, case_sensitive),
            None => Vec::new(),
        };
        self.find.matches = matches;
        if self.find.current >= self.find.matches.len() {
            self.find.current = 0;
        }

        self.highlight_preview(cx);
        self.apply_find_position(cx);
        cx.notify();
    }

    /// 演示视图里把所有命中加上底色，并滚到当前命中
    fn highlight_preview(&mut self, cx: &mut Context<Self>) {
        let query = self.find.input.read(cx).value().to_string();
        let case_sensitive = self.find.case_sensitive;
        let current = self.find.current;

        let Some(ix) = self.active else {
            return;
        };
        let Some(Tab::File(doc)) = self.tabs.get_mut(ix) else {
            return;
        };

        // 演示视图不在屏幕上时不用重建，清掉旧高亮就够
        if !matches!(doc.view, ViewMode::Preview | ViewMode::Split) {
            doc.preview
                .update(cx, |state, cx| state.clear_range_highlights(cx));
            return;
        }

        // 先让演示视图跟上文档的最新文本，否则算出的源区间对不上渲染结果
        let src = preview_source(&doc.text(cx));
        if doc.preview_src != src {
            doc.preview_src = src.clone();
            doc.preview.update(cx, |state, cx| state.set_text(&src, cx));
        }

        let ranges = search::occurrences(&doc.preview_src, &query, case_sensitive);
        let target = ranges.get(current).copied();
        let background = cx.theme().selection;
        doc.preview.update(cx, |state, cx| {
            let rendered = state.rendered_text();
            let highlights: Vec<RangeHighlight> = ranges
                .iter()
                .filter_map(|(start, end)| rendered.range_for_source(*start..*end))
                .map(|range| RangeHighlight::new(range, background))
                .collect();
            let _ = state.set_range_highlights(highlights, cx);
            if let Some(range) =
                target.and_then(|(start, end)| rendered.range_for_source(start..end))
            {
                let _ = state.reveal_range(range, cx);
            }
        });
    }

    /// 把当前命中定位到编辑器里（演示视图由 highlight_preview 负责滚动）
    fn apply_find_position(&mut self, cx: &mut Context<Self>) {
        let query = self.find.input.read(cx).value().to_string();
        let case_sensitive = self.find.case_sensitive;
        let index = self.find.current;
        let range = self.find.matches.get(index).copied();

        let Some(ix) = self.active else {
            return;
        };
        let Some(Tab::File(doc)) = self.tabs.get_mut(ix) else {
            return;
        };

        match doc.view {
            ViewMode::Preview => {}
            ViewMode::Source | ViewMode::Split => {
                if let Some((start, end)) = range {
                    // 命中是上次刷新时算的，文本可能已经改过，越界先跳过
                    if doc.editor.read(cx).value().get(start..end).is_some() {
                        doc.editor
                            .update(cx, |state, cx| state.set_selected_range(start..end, cx));
                    }
                }
            }
            ViewMode::Edit => {
                // 块编辑器的选中位置按「块 + 字符偏移」记，这里按文档顺序把
                // 源文本里的第 index 个命中换算成块内位置
                let mut remaining = index;
                let mut target: Option<(BlockId, usize, usize)> = None;
                {
                    let state = doc.wysiwyg.read(cx);
                    for block in state.model().document().leaves() {
                        let Some(text) = block.text() else {
                            continue;
                        };
                        for (start, end) in search::occurrences(text, &query, case_sensitive) {
                            if remaining == 0 {
                                target = Some((
                                    block.id(),
                                    text[..start].chars().count(),
                                    text[..end].chars().count(),
                                ));
                                break;
                            }
                            remaining -= 1;
                        }
                        if target.is_some() {
                            break;
                        }
                    }
                }
                if let Some((block, anchor, head)) = target {
                    doc.wysiwyg.update(cx, |state, cx| {
                        let selection = BlockSelection::Text {
                            anchor: BlockPosition::new(block, anchor),
                            head: BlockPosition::new(block, head),
                        };
                        if state.set_selection(selection, cx).is_ok() {
                            state.reveal_caret(cx);
                        }
                    });
                }
            }
        }
    }

    fn find_step(&mut self, forward: bool, cx: &mut Context<Self>) {
        let count = self.find.matches.len();
        if count == 0 {
            return;
        }
        self.find.current = if forward {
            (self.find.current + 1) % count
        } else {
            (self.find.current + count - 1) % count
        };
        self.highlight_preview(cx);
        self.apply_find_position(cx);
        cx.notify();
    }

    fn clear_preview_highlights(&mut self, cx: &mut Context<Self>) {
        let Some(doc) = self.active_doc() else {
            return;
        };
        doc.preview
            .update(cx, |state, cx| state.clear_range_highlights(cx));
    }

    /// 替换后写回两个编辑器，并让演示视图在下次渲染时重新同步
    fn set_doc_text(&mut self, text: String, window: &mut Window, cx: &mut Context<Self>) {
        let Some(ix) = self.active else {
            return;
        };
        let Some(Tab::File(doc)) = self.tabs.get_mut(ix) else {
            return;
        };
        doc.editor
            .update(cx, |state, cx| state.set_value(text.clone(), window, cx));
        doc.wysiwyg
            .update(cx, |state, cx| state.set_source(&text, cx).ok());
        doc.preview_src.clear();
        cx.notify();
    }

    /// 只替换当前这一处，替换完停在同一位置（也就是下一个命中）
    fn replace_current(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let query = self.find.input.read(cx).value().to_string();
        let replacement = self.find.replace.read(cx).value().to_string();
        if query.is_empty() {
            return;
        }
        let Some((start, end)) = self.find.matches.get(self.find.current).copied() else {
            self.set_status("没有可替换的命中", cx);
            return;
        };
        let Some(doc) = self.active_doc() else {
            return;
        };
        let text = doc.text(cx);
        // 命中是上次刷新时算的，文本若已改动，位置就作废了，重新算一遍
        if text.get(start..end).is_none() {
            self.refresh_find(cx);
            return;
        }

        let mut new_text = String::with_capacity(text.len() + replacement.len());
        new_text.push_str(&text[..start]);
        new_text.push_str(&replacement);
        new_text.push_str(&text[end..]);

        self.set_doc_text(new_text, window, cx);
        self.refresh_find(cx);
        self.set_status("已替换 1 处", cx);
    }

    fn replace_all_find(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let query = self.find.input.read(cx).value().to_string();
        let replacement = self.find.replace.read(cx).value().to_string();
        if query.is_empty() {
            return;
        }
        let case_sensitive = self.find.case_sensitive;
        let Some(doc) = self.active_doc() else {
            return;
        };
        let (new_text, count) =
            search::replace(&doc.text(cx), &query, &replacement, case_sensitive);
        if count == 0 {
            self.set_status("没有可替换的命中", cx);
            return;
        }

        self.set_doc_text(new_text, window, cx);
        self.refresh_find(cx);
        self.set_status(format!("已替换 {count} 处"), cx);
    }

    // ---------- Git ----------

    /// 弹出目录选择框，切换当前工作区
    fn pick_workspace(&mut self, cx: &mut Context<Self>) {
        let rx = cx.prompt_for_paths(PathPromptOptions {
            files: false,
            directories: true,
            multiple: false,
            prompt: Some("选择笔记目录".into()),
        });

        cx.spawn(async move |this, cx| {
            if let Ok(Ok(Some(mut paths))) = rx.await {
                if let Some(path) = paths.pop() {
                    this.update(cx, |this, cx| this.set_root(path, cx)).ok();
                }
            }
        })
        .detach();
    }

    /// 切换工作区：重建目录树并刷新源码管理面板
    fn set_root(&mut self, root: PathBuf, cx: &mut Context<Self>) {
        if root == self.root {
            return;
        }
        self.root = root;
        self.expanded.clear();

        // 旧工作区的搜索结果不再适用
        self.search.results.clear();
        self.search.total = 0;
        self.search.searching = false;
        self.search.truncated = false;
        self.search.generation += 1;

        // 新工作区为空时生成欢迎文档，等下一次渲染再打开
        if fs_ops::is_empty_dir(&self.root) {
            self.pending_open = fs_ops::write_welcome(&self.root).ok();
        }

        let root = self.root.clone();
        let expanded = self.expanded.clone();
        self.tree_state.update(cx, |state, cx| {
            state.set_items(fs_ops::build_items(&root, &expanded), cx);
        });

        self.refresh_git(cx);
        let label = self.root.display().to_string();
        self.persist_session();
        self.set_status(format!("工作区：{label}"), cx);
    }

    fn refresh_git(&mut self, cx: &mut Context<Self>) {
        let root = self.root.clone();
        self.git.loading = true;
        cx.notify();

        cx.spawn(async move |this, cx| {
            let snapshot = cx
                .background_executor()
                .spawn(async move { git::snapshot(&root) })
                .await;

            this.update(cx, |this, cx| {
                this.git.is_repo = snapshot.status.is_repo;
                this.git.branch = snapshot.status.branch;
                this.git.changes = snapshot.status.changes;
                this.git.log = snapshot.log;
                this.git.branches = snapshot.branches;
                this.git.loading = false;
                cx.notify();
            })
            .ok();
        })
        .detach();
    }

    /// 后台执行一个 git 写操作，完成后刷新面板
    fn run_git<F>(&mut self, label: &'static str, task: F, cx: &mut Context<Self>)
    where
        F: FnOnce() -> Result<String, String> + Send + 'static,
    {
        cx.spawn(async move |this, cx| {
            let result = cx.background_executor().spawn(async move { task() }).await;
            this.update(cx, |this, cx| {
                match result {
                    Ok(out) => {
                        let out = out.trim();
                        let message = if out.is_empty() {
                            format!("{label}完成")
                        } else {
                            format!("{label}：{}", first_line(out))
                        };
                        this.set_status(message, cx);
                    }
                    Err(err) => this.set_status(format!("{label}失败：{err}"), cx),
                }
                this.refresh_git(cx);
            })
            .ok();
        })
        .detach();
    }

    fn show_diff_tab(&mut self, title: SharedString, patch: String, cx: &mut Context<Self>) {
        match DiffFile::parse(&patch) {
            Ok(files) if !files.is_empty() => {
                let state = cx.new(|cx| DiffState::new(files, cx));
                self.tabs.push(Tab::Diff { title, state });
                self.active = Some(self.tabs.len() - 1);
                self.sync_selection(cx);
                cx.notify();
            }
            Ok(_) => self.set_status("这个改动没有可显示的差异", cx),
            Err(err) => self.set_status(format!("无法解析差异：{err}"), cx),
        }
    }

    /// 打开一个差异标签页（已存在则直接切换）
    fn open_diff_tab<F>(
        &mut self,
        title: SharedString,
        task: F,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) where
        F: FnOnce() -> Result<String, String> + Send + 'static,
    {
        if let Some(ix) = self.tabs.iter().position(
            |tab| matches!(tab, Tab::Diff { title: t, .. } if *t == title),
        ) {
            self.activate(ix, window, cx);
            return;
        }

        cx.spawn(async move |this, cx| {
            let result = cx.background_executor().spawn(async move { task() }).await;
            this.update(cx, |this, cx| match result {
                Ok(patch) => this.show_diff_tab(title, patch, cx),
                Err(err) => this.set_status(format!("读取差异失败：{err}"), cx),
            })
            .ok();
        })
        .detach();
    }

    fn open_change_diff(
        &mut self,
        change: git::Change,
        staged: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let label = if staged { "暂存" } else { "工作区" };
        let title: SharedString = format!("{} · {label}", change.path).into();
        let root = self.root.clone();
        let change_for_task = change.clone();
        self.open_diff_tab(
            title,
            move || git::diff(&root, &change_for_task, staged),
            window,
            cx,
        );
    }

    fn open_commit_diff(
        &mut self,
        hash: String,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let title: SharedString = format!("提交 {}", &hash).into();
        let root = self.root.clone();
        let hash_for_task = hash.clone();
        self.open_diff_tab(title, move || git::show(&root, &hash_for_task), window, cx);
    }

    fn on_stage(&mut self, change: git::Change, cx: &mut Context<Self>) {
        let root = self.root.clone();
        let path = change.path.clone();
        self.run_git("暂存", move || git::add(&root, &path), cx);
    }

    fn on_unstage(&mut self, change: git::Change, cx: &mut Context<Self>) {
        let root = self.root.clone();
        let path = change.path.clone();
        self.run_git("取消暂存", move || git::unstage(&root, &path), cx);
    }

    fn on_stage_all(&mut self, cx: &mut Context<Self>) {
        let root = self.root.clone();
        self.run_git("暂存全部", move || git::add_all(&root), cx);
    }

    fn on_unstage_all(&mut self, cx: &mut Context<Self>) {
        let root = self.root.clone();
        self.run_git("取消全部暂存", move || git::unstage_all(&root), cx);
    }

    fn on_discard(&mut self, change: git::Change, window: &mut Window, cx: &mut Context<Self>) {
        let view = cx.entity();
        let display = change.path.clone();
        window.open_dialog(cx, move |dialog, _, _| {
            let view = view.clone();
            let change = change.clone();
            dialog
                .title("丢弃改动")
                .child(SharedString::from(format!(
                    "确定丢弃“{display}”的改动吗？此操作不可恢复。"
                )))
                .footer(
                    DialogFooter::new()
                        .child(
                            Button::new("cancel")
                                .label("取消")
                                .on_click(|_, window, cx| window.close_dialog(cx)),
                        )
                        .child(
                            DialogAction::new()
                                .child(Button::new("ok").danger().label("丢弃")),
                        ),
                )
                .on_ok(move |_, _, cx| {
                    let view = view.clone();
                    let change = change.clone();
                    view.update(cx, |this, cx| {
                        let root = this.root.clone();
                        let change_for_task = change.clone();
                        this.run_git("丢弃", move || git::discard(&root, &change_for_task), cx);
                    });
                    true
                })
        });
    }

    fn on_commit(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let message = self.git.input.read(cx).value().trim().to_string();
        if message.is_empty() {
            self.set_status("提交说明不能为空", cx);
            return;
        }
        if self.git.changes.iter().all(|change| !change.staged()) {
            self.set_status("暂存区是空的，先暂存要提交的改动", cx);
            return;
        }

        let input = self.git.input.clone();
        input.update(cx, |state, cx| state.set_value("", window, cx));

        let root = self.root.clone();
        self.run_git("提交", move || git::commit(&root, &message), cx);
    }

    fn on_checkout(&mut self, branch: String, cx: &mut Context<Self>) {
        let root = self.root.clone();
        self.show_branches = false;
        self.run_git("切换分支", move || git::checkout(&root, &branch), cx);
    }

    fn on_push(&mut self, cx: &mut Context<Self>) {
        let root = self.root.clone();
        self.run_git("推送", move || git::push(&root), cx);
    }

    fn on_pull(&mut self, cx: &mut Context<Self>) {
        let root = self.root.clone();
        self.run_git("拉取", move || git::pull(&root), cx);
    }

    fn on_init_repo(&mut self, cx: &mut Context<Self>) {
        let root = self.root.clone();
        self.run_git("初始化仓库", move || git::init(&root), cx);
    }

    // ---------- 渲染：侧栏 ----------

    fn render_sidebar(&mut self, cx: &mut Context<Self>) -> AnyElement {
        let border = cx.theme().border;
        let explorer = self.sidebar == SidebarView::Explorer;
        let view = cx.entity();

        let body = match self.sidebar {
            SidebarView::Explorer => self.render_explorer(cx),
            SidebarView::Search => self.render_search(cx),
            SidebarView::SourceControl => self.render_git_panel(cx),
        };

        v_flex()
            .w(px(self.sidebar_width))
            .h_full()
            .flex_shrink_0()
            .child(
                h_flex()
                    .px_1p5()
                    .py_1()
                    .gap_1()
                    .items_center()
                    .border_b_1()
                    .border_color(border)
                    .child(
                        Button::new("view-explorer")
                            .ghost()
                            .small()
                            .selected(explorer)
                            .icon(IconName::Files)
                            .tooltip("资源管理器")
                            .on_click({
                                let view = view.clone();
                                move |_, _, cx| {
                                    view.update(cx, |this, cx| {
                                        this.sidebar = SidebarView::Explorer;
                                        cx.notify();
                                    })
                                }
                            }),
                    )
                    .child(
                        Button::new("view-search")
                            .ghost()
                            .small()
                            .selected(self.sidebar == SidebarView::Search)
                            .icon(IconName::Search)
                            .tooltip("搜索（Ctrl+Shift+F）")
                            .on_click({
                                let view = view.clone();
                                move |_, window, cx| {
                                    view.update(cx, |this, cx| this.show_search(window, cx));
                                }
                            }),
                    )
                    .child(
                        Button::new("view-git")
                            .ghost()
                            .small()
                            .selected(self.sidebar == SidebarView::SourceControl)
                            .icon(IconName::GitBranch)
                            .tooltip("源代码管理")
                            .on_click({
                                let view = view.clone();
                                move |_, _, cx| {
                                    view.update(cx, |this, cx| {
                                        this.sidebar = SidebarView::SourceControl;
                                        cx.notify();
                                    })
                                }
                            }),
                    )
                    .child(div().flex_1())
                    .child(
                        Button::new("sidebar-switch-workspace")
                            .ghost()
                            .small()
                            .icon(IconName::FolderRoot)
                            .tooltip("切换工作区")
                            .on_click({
                                let view = view.clone();
                                move |_, _, cx| {
                                    view.update(cx, |this, cx| this.pick_workspace(cx));
                                }
                            }),
                    )
                    .child(
                        Button::new("sidebar-reveal")
                            .ghost()
                            .small()
                            .icon(IconName::FolderOpen)
                            .tooltip("在资源管理器中打开当前工作区")
                            .on_click({
                                let view = view.clone();
                                move |_, _, cx| {
                                    view.update(cx, |this, cx| {
                                        fs_ops::reveal_in_explorer(&this.root);
                                        this.set_status("已在资源管理器中打开当前工作区", cx);
                                    })
                                }
                            }),
                    ),
            )
            .child(body)
            .into_any_element()
    }

    /// 资源管理器：顶部工具栏 + 目录树
    fn render_explorer(&mut self, cx: &mut Context<Self>) -> AnyElement {
        let muted = cx.theme().muted_foreground;
        let root = self.root.clone();
        let view = cx.entity();
        let menu_view = view.clone();
        let menu_root = root.clone();
        // 目录树空白区域右键用：默认在笔记根目录下操作
        let blank_view = view.clone();
        let blank_root = root.clone();
        let tree_state = self.tree_state.clone();

        let selected = self.selected_path(cx);
        // 与右键菜单一致：选中目录就在目录里新建，选中文件就在它旁边新建
        let target = match selected.clone() {
            Some(path) if path.is_dir() => path,
            Some(path) => path
                .parent()
                .map(Path::to_path_buf)
                .unwrap_or_else(|| root.clone()),
            None => root.clone(),
        };

        // 已打开且有未保存改动的文件，用于在树上打点
        let dirty_paths: HashSet<String> = self
            .tabs
            .iter()
            .filter(|tab| tab.dirty(cx))
            .filter_map(|tab| tab.path().map(|p| p.to_string_lossy().to_string()))
            .collect();

        let open_dirty = dirty_paths.clone();
        let tree_view = view.clone();
        let focus_state = self.tree_state.clone();
        let menu_tree = self.tree_state.clone();
        let tree_icons = self.icons.clone();
        let expanded_now = self.expanded.clone();
        // 内联输入所在的行（新建的占位行或正在重命名的行）
        let inline_row = self.inline.as_ref().map(|edit| InlineRow {
            id: SharedString::from(edit.row_id()),
            kind: edit.kind,
            input: edit.input.clone(),
        });
        let tree_element = tree(&tree_state, move |ix, entry, _selected, _window, _cx| {
            let id = entry.item().id.to_string();

            if let Some(inline) = inline_row.as_ref().filter(|row| &*row.id == id) {
                let icon = match inline.kind {
                    InlineKind::NewFolder => tree_icons.folder(true),
                    InlineKind::Rename => {
                        let path = PathBuf::from(&id);
                        if path.is_dir() {
                            tree_icons.folder(entry.is_folder() && entry.is_expanded())
                        } else {
                            tree_icons.file(&path)
                        }
                    }
                    InlineKind::NewFile => tree_icons.file(Path::new("笔记.md")),
                };
                let commit_view = tree_view.clone();
                let cancel_view = tree_view.clone();
                return ListItem::new(ix)
                    .pl(px(entry.depth() as f32 * 14. + 8.))
                    .child(
                        h_flex()
                            .w_full()
                            .gap_2()
                            .items_center()
                            // 输入框里的点击 / 键盘不要冒泡成树行的选中、展开和回车
                            .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
                            .id("tree-inline-edit")
                            .on_click(|_, _, cx| cx.stop_propagation())
                            .key_context("InlineEdit")
                            .on_action(move |_: &InlineCommit, window, cx| {
                                commit_view.update(cx, |this, cx| this.commit_inline(window, cx))
                            })
                            .on_action(move |_: &InlineCancel, _window, cx| {
                                cancel_view.update(cx, |this, cx| this.cancel_inline(cx))
                            })
                            .child(icon)
                            .child(
                                div()
                                    .flex_1()
                                    .min_w_0()
                                    .child(Input::new(&inline.input).small()),
                            ),
                    );
            }

            let path = PathBuf::from(&id);
            // 空文件夹没有子项，树会把它当叶子，这里再问一下磁盘
            let is_folder = entry.is_folder() || path.is_dir();
            // 空文件夹在树里是叶子，没有展开状态，用自己记的那份让它也能切换图标
            let expanded = if entry.is_folder() {
                entry.is_expanded()
            } else {
                expanded_now.contains(&id)
            };
            let opened = !is_folder && open_dirty.contains(&id);

            let icon = if is_folder {
                tree_icons.folder(expanded)
            } else {
                tree_icons.file(&path)
            };

            // ListItem 内部是块级容器，多个子元素会纵向堆叠，这里统一包一层横向布局
            let mut row = h_flex()
                .w_full()
                .gap_2()
                .items_center()
                .child(icon)
                .child(
                    div()
                        .flex_1()
                        .min_w_0()
                        .text_sm()
                        .truncate()
                        .child(entry.item().label.clone()),
                );
            if opened {
                row = row.child(div().text_xs().text_color(muted).child("●"));
            }
            let mut item = ListItem::new(ix)
                .pl(px(entry.depth() as f32 * 14. + 8.))
                .child(row);
            let click_view = tree_view.clone();
            let click_path = path.clone();
            let click_state = focus_state.clone();
            // 空文件夹在树里是叶子，点击时自己切换展开状态，让图标有开合反馈
            let click_empty_folder = is_folder && !entry.is_folder();
            let click_id = id.clone();
            item = item.on_click(move |_, window, cx| {
                // 点击文件夹时把焦点交给目录树，方便继续用 F2 / Delete
                if is_folder {
                    click_state.update(cx, |state, cx| state.focus(window, cx));
                }
                click_view.update(cx, |this, cx| {
                    if click_empty_folder {
                        if !this.expanded.remove(&click_id) {
                            this.expanded.insert(click_id.clone());
                        }
                        cx.notify();
                    }
                    this.open_file(click_path.clone(), window, cx)
                });
            });
            item
        })
        .context_menu(move |ix, entry, menu, window, cx| {
            // 右键时同步选中该行（延后执行，避免与目录树自身的更新冲突）
            let handle = window.window_handle();
            let tree_for_select = menu_tree.clone();
            cx.defer(move |cx| {
                let _ = handle.update(cx, |_, _window, cx| {
                    tree_for_select.update(cx, |state, cx| state.set_selected_index(Some(ix), cx));
                });
            });
            let path = PathBuf::from(entry.item().id.to_string());
            let is_dir = path.is_dir();
            let parent = if is_dir {
                path.clone()
            } else {
                path.parent().unwrap_or(&menu_root).to_path_buf()
            };
            let can_modify = path != menu_root;

            let mut menu = menu.item(
                PopupMenuItem::new("新建笔记")
                    .icon(IconName::FilePlus)
                    .on_click({
                        let view = menu_view.clone();
                        let parent = parent.clone();
                        move |_, window, cx| {
                            view.update(cx, |this, cx| {
                                this.start_inline(InlineKind::NewFile, parent.clone(), window, cx)
                            })
                        }
                    }),
            );
            menu = menu.item(
                PopupMenuItem::new("新建文件夹")
                    .icon(IconName::FolderPlus)
                    .on_click({
                        let view = menu_view.clone();
                        let parent = parent.clone();
                        move |_, window, cx| {
                            view.update(cx, |this, cx| {
                                this.start_inline(InlineKind::NewFolder, parent.clone(), window, cx)
                            })
                        }
                    }),
            );
            menu = menu.separator();
            menu = menu.item(
                PopupMenuItem::new("重命名")
                    .icon(IconName::Pencil)
                    .disabled(!can_modify)
                    .on_click({
                        let view = menu_view.clone();
                        let path = path.clone();
                        move |_, window, cx| {
                            view.update(cx, |this, cx| {
                                this.start_inline(InlineKind::Rename, path.clone(), window, cx)
                            })
                        }
                    }),
            );
            menu = menu.item(
                PopupMenuItem::new("删除")
                    .icon(IconName::Trash)
                    .disabled(!can_modify)
                    .on_click({
                        let view = menu_view.clone();
                        let path = path.clone();
                        move |_, window, cx| {
                            view.update(cx, |this, cx| this.confirm_delete(path.clone(), window, cx))
                        }
                    }),
            );
            menu = menu.separator();
            menu.item(
                PopupMenuItem::new("在资源管理器中显示").icon(IconName::FolderOpen).on_click({
                    let target = if is_dir {
                        path.clone()
                    } else {
                        path.parent().unwrap_or(&menu_root).to_path_buf()
                    };
                    move |_, _, _cx| fs_ops::reveal_in_explorer(&target)
                }),
            )
        });

        v_flex()
            .flex_1()
            .min_h_0()
            .child(
                h_flex()
                    .px_2()
                    .py_1()
                    .gap_0p5()
                    .items_center()
                    .justify_between()
                    .child(
                        div()
                            .text_xs()
                            .font_semibold()
                            .text_color(muted)
                            .child("资源管理器"),
                    )
                    .child(
                        h_flex()
                            .gap_0p5()
                            .child(
                                Button::new("new-file")
                                    .ghost()
                                    .small()
                                    .icon(IconName::FilePlus)
                                    .tooltip("新建笔记")
                                    .on_click({
                                        let view = view.clone();
                                        let target = target.clone();
                                        move |_, window, cx| {
                                            view.update(cx, |this, cx| {
                                                this.start_inline(
                                                    InlineKind::NewFile,
                                                    target.clone(),
                                                    window,
                                                    cx,
                                                )
                                            })
                                        }
                                    }),
                            )
                            .child(
                                Button::new("new-folder")
                                    .ghost()
                                    .small()
                                    .icon(IconName::FolderPlus)
                                    .tooltip("新建文件夹")
                                    .on_click({
                                        let view = view.clone();
                                        let target = target.clone();
                                        move |_, window, cx| {
                                            view.update(cx, |this, cx| {
                                                this.start_inline(
                                                    InlineKind::NewFolder,
                                                    target.clone(),
                                                    window,
                                                    cx,
                                                )
                                            })
                                        }
                                    }),
                            )
                            .child(
                                Button::new("rename-item")
                                    .ghost()
                                    .small()
                                    .icon(IconName::Pencil)
                                    .tooltip("重命名（F2）")
                                    .disabled(selected.is_none() || selected.as_ref() == Some(&root))
                                    .on_click({
                                        let view = view.clone();
                                        move |_, window, cx| {
                                            view.update(cx, |this, cx| {
                                                this.on_rename_selected(
                                                    &RenameSelected,
                                                    window,
                                                    cx,
                                                )
                                            })
                                        }
                                    }),
                            )
                            .child(
                                Button::new("delete-item")
                                    .ghost()
                                    .small()
                                    .icon(IconName::Trash)
                                    .tooltip("删除")
                                    .disabled(selected.is_none() || selected.as_ref() == Some(&root))
                                    .on_click({
                                        let view = view.clone();
                                        move |_, window, cx| {
                                            view.update(cx, |this, cx| {
                                                this.on_delete_selected(
                                                    &DeleteSelected,
                                                    window,
                                                    cx,
                                                )
                                            })
                                        }
                                    }),
                            )
                            .child(
                                Button::new("refresh-tree")
                                    .ghost()
                                    .small()
                                    .icon(IconName::RefreshCw)
                                    .tooltip("刷新（F5）")
                                    .on_click({
                                        let view = view.clone();
                                        move |_, window, cx| {
                                            view.update(cx, |this, cx| {
                                                this.on_refresh(&Refresh, window, cx)
                                            })
                                        }
                                    }),
                            ),
                    ),
            )
            .child(
                div()
                    .id("explorer-blank")
                    .flex_1()
                    .min_h_0()
                    .overflow_hidden()
                    // 树的行自带右键菜单并会 stop_propagation，这里只处理空白区域
                    .context_menu(move |menu, _window, _cx| {
                        let mut menu = menu.item(
                            PopupMenuItem::new("新建笔记")
                                .icon(IconName::FilePlus)
                                .on_click({
                                    let view = blank_view.clone();
                                    let root = blank_root.clone();
                                    move |_, window, cx| {
                                        view.update(cx, |this, cx| {
                                            this.start_inline(
                                                InlineKind::NewFile,
                                                root.clone(),
                                                window,
                                                cx,
                                            )
                                        })
                                    }
                                }),
                        );
                        menu = menu.item(
                            PopupMenuItem::new("新建文件夹")
                                .icon(IconName::FolderPlus)
                                .on_click({
                                    let view = blank_view.clone();
                                    let root = blank_root.clone();
                                    move |_, window, cx| {
                                        view.update(cx, |this, cx| {
                                            this.start_inline(
                                                InlineKind::NewFolder,
                                                root.clone(),
                                                window,
                                                cx,
                                            )
                                        })
                                    }
                                }),
                        );
                        menu = menu.separator();
                        menu = menu.item(
                            PopupMenuItem::new("刷新")
                                .icon(IconName::RefreshCw)
                                .on_click({
                                    let view = blank_view.clone();
                                    move |_, window, cx| {
                                        view.update(cx, |this, cx| {
                                            this.on_refresh(&Refresh, window, cx)
                                        })
                                    }
                                }),
                        );
                        menu.item(
                            PopupMenuItem::new("在资源管理器中显示")
                                .icon(IconName::FolderOpen)
                                .on_click({
                                    let root = blank_root.clone();
                                    move |_, _, _cx| fs_ops::reveal_in_explorer(&root)
                                }),
                        )
                    })
                    .child(tree_element),
            )
            .into_any_element()
    }

    // ---------- 渲染：搜索 ----------

    fn render_search(&mut self, cx: &mut Context<Self>) -> AnyElement {
        let border = cx.theme().border;
        let muted = cx.theme().muted_foreground;
        let selection = cx.theme().selection;
        let view = cx.entity();

        let input = self.search.input.clone();
        let replace_input = self.search.replace.clone();
        let case_sensitive = self.search.case_sensitive;
        let query = self.search.query.clone();
        let searching = self.search.searching;

        let form = v_flex()
            .p_2()
            .gap_1()
            .border_b_1()
            .border_color(border)
            .child(
                h_flex()
                    .gap_1()
                    .items_center()
                    .child(
                        div()
                            .flex_1()
                            .min_w_0()
                            .child(Input::new(&input).cleanable(true)),
                    )
                    .child(
                        Button::new("search-case")
                            .ghost()
                            .small()
                            .selected(case_sensitive)
                            .icon(IconName::CaseSensitive)
                            .tooltip("区分大小写")
                            .on_click({
                                let view = view.clone();
                                move |_, _, cx| {
                                    view.update(cx, |this, cx| {
                                        this.search.case_sensitive = !this.search.case_sensitive;
                                        this.start_search(cx);
                                    })
                                }
                            }),
                    ),
            )
            .child(
                h_flex()
                    .gap_1()
                    .items_center()
                    .child(div().flex_1().min_w_0().child(Input::new(&replace_input)))
                    .child(
                        Button::new("search-replace-all")
                            .ghost()
                            .small()
                            .icon(IconName::Replace)
                            .tooltip("全部替换")
                            .on_click({
                                let view = view.clone();
                                move |_, window, cx| {
                                    view.update(cx, |this, cx| this.confirm_replace(window, cx))
                                }
                            }),
                    ),
            );

        let mut rows: Vec<AnyElement> = Vec::new();

        if query.trim().is_empty() {
            rows.push(
                div()
                    .px_3()
                    .py_4()
                    .text_xs()
                    .text_color(muted)
                    .child("输入关键词，在当前工作区里查找")
                    .into_any_element(),
            );
        } else if self.search.results.is_empty() {
            rows.push(
                div()
                    .px_3()
                    .py_4()
                    .text_xs()
                    .text_color(muted)
                    .child(if searching { "搜索中…" } else { "没有匹配的内容" })
                    .into_any_element(),
            );
        }

        for (file_ix, file) in self.search.results.iter().enumerate() {
            let rel = file
                .path
                .strip_prefix(&self.root)
                .unwrap_or(&file.path)
                .to_string_lossy()
                .replace('\\', "/");
            rows.push(
                h_flex()
                    .px_2()
                    .pt_2()
                    .pb_1()
                    .gap_1()
                    .items_center()
                    .child(self.icons.file(&file.path))
                    .child(
                        div()
                            .flex_1()
                            .min_w_0()
                            .text_xs()
                            .font_semibold()
                            .truncate()
                            .child(rel),
                    )
                    .child(
                        div()
                            .flex_shrink_0()
                            .text_xs()
                            .text_color(muted)
                            .child(file.hits.len().to_string()),
                    )
                    .into_any_element(),
            );

            for (hit_ix, hit) in file.hits.iter().enumerate() {
                // 行首缩进不显示，命中区间跟着平移
                let lead = hit.text.len() - hit.text.trim_start().len();
                let text = &hit.text[lead..];
                let start = hit.start.saturating_sub(lead);
                let end = hit.end.saturating_sub(lead).max(start);
                let before = text.get(..start).unwrap_or("").to_string();
                let matched = text.get(start..end).unwrap_or("").to_string();
                let after = text.get(end..).unwrap_or("").to_string();

                rows.push(
                    ListItem::new(format!("search-hit-{file_ix}-{hit_ix}"))
                        .pl(px(22.))
                        .pr(px(8.))
                        .child(
                            h_flex()
                                .w_full()
                                .gap_2()
                                .items_center()
                                .child(
                                    div()
                                        .w(px(26.))
                                        .flex_shrink_0()
                                        .text_xs()
                                        .text_color(muted)
                                        .child(hit.line.to_string()),
                                )
                                .child(
                                    h_flex()
                                        .flex_1()
                                        .min_w_0()
                                        .overflow_hidden()
                                        .text_xs()
                                        .child(div().min_w_0().truncate().child(before))
                                        .child(
                                            div()
                                                .flex_shrink_0()
                                                .rounded(px(2.))
                                                .bg(selection)
                                                .child(matched),
                                        )
                                        .child(div().min_w_0().truncate().child(after)),
                                ),
                        )
                        .on_click({
                            let view = view.clone();
                            let path = file.path.clone();
                            let line = hit.line;
                            move |_, window, cx| {
                                let path = path.clone();
                                view.update(cx, |this, cx| {
                                    this.jump_to_hit(path.clone(), line, window, cx)
                                });
                            }
                        })
                        .into_any_element(),
                );
            }
        }

        let summary = if !query.trim().is_empty() && !self.search.results.is_empty() {
            let mut text = format!("{} 处结果", self.search.total);
            if self.search.truncated {
                text.push_str("（结果过多，仅显示一部分）");
            }
            Some(text)
        } else {
            None
        };

        let mut list = v_flex()
            .id("search-results")
            .flex_1()
            .min_h_0()
            .overflow_y_scroll();
        if let Some(summary) = summary {
            list = list.child(
                div()
                    .px_2()
                    .py_1()
                    .text_xs()
                    .text_color(muted)
                    .child(summary),
            );
        }

        v_flex()
            .flex_1()
            .min_h_0()
            .child(form)
            .child(list.children(rows))
            .into_any_element()
    }

    // ---------- 渲染：源代码管理 ----------

    fn render_git_panel(&mut self, cx: &mut Context<Self>) -> AnyElement {
        let border = cx.theme().border;
        let muted = cx.theme().muted_foreground;
        let success = cx.theme().success;
        let warning = cx.theme().warning;
        let view = cx.entity();

        if !self.git.is_repo {
            return v_flex()
                .flex_1()
                .min_h_0()
                .items_center()
                .justify_center()
                .gap_3()
                .p_4()
                .child(Icon::new(IconName::GitBranch).size_8().text_color(muted))
                .child(
                    div()
                        .text_sm()
                        .text_color(muted)
                        .text_center()
                        .child("笔记目录还不是 Git 仓库"),
                )
                .child(
                    Button::new("git-init")
                        .primary()
                        .small()
                        .label("初始化仓库")
                        .on_click({
                            let view = view.clone();
                            move |_, _, cx| {
                                view.update(cx, |this, cx| this.on_init_repo(cx));
                            }
                        }),
                )
                .into_any_element();
        }

        let branch_label: SharedString = if self.git.branch.is_empty() {
            "（无分支）".into()
        } else {
            self.git.branch.clone().into()
        };
        let show_branches = self.show_branches;
        let loading = self.git.loading;

        let branch_row = h_flex()
            .px_2()
            .py_1()
            .gap_1()
            .items_center()
            .border_b_1()
            .border_color(border)
            .child(
                Button::new("git-branch")
                    .ghost()
                    .small()
                    .icon(IconName::GitBranch)
                    .label(branch_label)
                    .tooltip("切换分支")
                    .on_click({
                        let view = view.clone();
                        move |_, _, cx| {
                            view.update(cx, |this, cx| {
                                this.show_branches = !this.show_branches;
                                cx.notify();
                            })
                        }
                    }),
            )
            .child(div().flex_1())
            .child(
                Button::new("git-pull")
                    .ghost()
                    .small()
                    .icon(IconName::ArrowDown)
                    .tooltip("拉取（pull）")
                    .disabled(loading)
                    .on_click({
                        let view = view.clone();
                        move |_, _, cx| view.update(cx, |this, cx| this.on_pull(cx))
                    }),
            )
            .child(
                Button::new("git-push")
                    .ghost()
                    .small()
                    .icon(IconName::ArrowUp)
                    .tooltip("推送（push）")
                    .disabled(loading)
                    .on_click({
                        let view = view.clone();
                        move |_, _, cx| view.update(cx, |this, cx| this.on_push(cx))
                    }),
            )
            .child(
                Button::new("git-refresh")
                    .ghost()
                    .small()
                    .icon(IconName::RefreshCw)
                    .tooltip("刷新（F5）")
                    .on_click({
                        let view = view.clone();
                        move |_, _, cx| view.update(cx, |this, cx| this.refresh_git(cx))
                    }),
            );

        // 分支列表
        let branches_popup = if show_branches {
            let rows: Vec<AnyElement> = self
                .git
                .branches
                .iter()
                .enumerate()
                .map(|(ix, branch)| {
                    let current = *branch == self.git.branch;
                    let branch = branch.clone();
                    let view = view.clone();
                    div()
                        .child(
                            ListItem::new(format!("branch-{ix}"))
                                .selected(current)
                                .child(Icon::new(IconName::GitBranch).size_4())
                                .child(div().flex_1().text_sm().truncate().child(branch.clone()))
                                .on_click(move |_, _, cx| {
                                    let branch = branch.clone();
                                    view.update(cx, |this, cx| this.on_checkout(branch, cx));
                                }),
                        )
                        .into_any_element()
                })
                .collect();
            Some(
                div()
                    .id("git-branches")
                    .max_h(px(160.))
                    .overflow_y_scroll()
                    .border_b_1()
                    .border_color(border)
                    .children(rows),
            )
        } else {
            None
        };

        // 提交区
        let input = self.git.input.clone();
        let commit_box = v_flex()
            .p_2()
            .gap_1()
            .border_b_1()
            .border_color(border)
            .child(Input::new(&input))
            .child(
                Button::new("git-commit")
                    .primary()
                    .small()
                    .w_full()
                    .label("提交")
                    .on_click({
                        let view = view.clone();
                        move |_, window, cx| {
                            view.update(cx, |this, cx| this.on_commit(window, cx))
                        }
                    }),
            );

        // 改动列表
        let staged: Vec<git::Change> = self
            .git
            .changes
            .iter()
            .filter(|change| change.staged())
            .cloned()
            .collect();
        let unstaged: Vec<git::Change> = self
            .git
            .changes
            .iter()
            .filter(|change| !change.staged())
            .cloned()
            .collect();

        let mut change_rows: Vec<AnyElement> = Vec::new();

        if !staged.is_empty() || !unstaged.is_empty() {
            change_rows.push(
                h_flex()
                    .px_2()
                    .py_1()
                    .items_center()
                    .gap_1()
                    .child(
                        div()
                            .flex_1()
                            .text_xs()
                            .font_semibold()
                            .text_color(muted)
                            .child(format!("暂存的更改（{}）", staged.len())),
                    )
                    .child(
                        Button::new("git-unstage-all")
                            .ghost()
                            .small()
                            .icon(IconName::Minus)
                            .tooltip("取消全部暂存")
                            .disabled(staged.is_empty())
                            .on_click({
                                let view = view.clone();
                                move |_, _, cx| {
                                    view.update(cx, |this, cx| this.on_unstage_all(cx))
                                }
                            }),
                    )
                    .into_any_element(),
            );

            for (ix, change) in staged.iter().enumerate() {
                change_rows.push(self.render_change_row(
                    ix,
                    change.clone(),
                    true,
                    success,
                    &view,
                ));
            }

            change_rows.push(
                h_flex()
                    .px_2()
                    .py_1()
                    .items_center()
                    .gap_1()
                    .child(
                        div()
                            .flex_1()
                            .text_xs()
                            .font_semibold()
                            .text_color(muted)
                            .child(format!("更改（{}）", unstaged.len())),
                    )
                    .child(
                        Button::new("git-stage-all")
                            .ghost()
                            .small()
                            .icon(IconName::Plus)
                            .tooltip("暂存全部改动")
                            .disabled(unstaged.is_empty())
                            .on_click({
                                let view = view.clone();
                                move |_, _, cx| {
                                    view.update(cx, |this, cx| this.on_stage_all(cx))
                                }
                            }),
                    )
                    .into_any_element(),
            );

            for (ix, change) in unstaged.iter().enumerate() {
                change_rows.push(self.render_change_row(
                    ix + 1000,
                    change.clone(),
                    false,
                    warning,
                    &view,
                ));
            }
        } else {
            change_rows.push(
                div()
                    .px_3()
                    .py_4()
                    .text_xs()
                    .text_color(muted)
                    .child("没有未提交的改动")
                    .into_any_element(),
            );
        }

        // 历史
        let mut history_rows: Vec<AnyElement> = Vec::new();
        if !self.git.log.is_empty() {
            history_rows.push(
                div()
                    .px_2()
                    .py_1()
                    .text_xs()
                    .font_semibold()
                    .text_color(muted)
                    .child("历史提交")
                    .into_any_element(),
            );
            for (ix, commit) in self.git.log.iter().enumerate() {
                let hash = commit.hash.clone();
                let label = format!("{} {}", commit.hash, commit.subject);
                let date = commit.date.clone();
                let view = view.clone();
                history_rows.push(
                    div()
                        .child(
                            ListItem::new(format!("commit-{ix}"))
                                .child(Icon::new(IconName::GitCommitHorizontal).size_4())
                                .child(div().flex_1().text_xs().truncate().child(label))
                                .suffix(move |_, _| {
                                    div().text_xs().text_color(muted).child(date.clone())
                                })
                                .on_click(move |_, window, cx| {
                                    let hash = hash.clone();
                                    view.update(cx, |this, cx| {
                                        this.open_commit_diff(hash.clone(), window, cx)
                                    });
                                }),
                        )
                        .into_any_element(),
                );
            }
        }

        v_flex()
            .flex_1()
            .min_h_0()
            .on_drag_move(cx.listener(
                |this, event: &DragMoveEvent<HistoryDrag>, _, cx| {
                    let panel_height = f32::from(event.bounds.size.height);
                    if panel_height <= 0. {
                        return;
                    }
                    let bottom = f32::from(event.bounds.origin.y) + panel_height;
                    let pointer = f32::from(event.event.position.y);
                    // 给上方的改动列表留出空间，避免把历史拖到没有位置
                    let max = (panel_height - 160.).max(MIN_HISTORY);
                    let height = (bottom - pointer).clamp(MIN_HISTORY, max);
                    if (this.history_height - height).abs() > 0.5 {
                        this.history_height = height;
                        cx.notify();
                    }
                },
            ))
            .child(branch_row)
            .children(branches_popup)
            .child(commit_box)
            .child(
                div()
                    .id("git-changes")
                    .flex_1()
                    .min_h_0()
                    .overflow_y_scroll()
                    .children(change_rows),
            )
            .child(
                div()
                    .id("git-history-divider")
                    .h(px(5.))
                    .w_full()
                    .flex_shrink_0()
                    .bg(border)
                    .cursor(CursorStyle::ResizeUpDown)
                    .on_drag(HistoryDrag, |_, _, _, cx| cx.new(|_| SplitGhost)),
            )
            .child(
                div()
                    .id("git-history")
                    .h(px(self.history_height))
                    .flex_shrink_0()
                    .overflow_y_scroll()
                    .children(history_rows),
            )
            .into_any_element()
    }

    /// 一条改动记录：点击看差异，右键暂存 / 丢弃
    fn render_change_row(
        &mut self,
        id: usize,
        change: git::Change,
        staged: bool,
        code_color: Hsla,
        view: &Entity<Self>,
    ) -> AnyElement {
        let code_label = if staged {
            change.staged_code()
        } else {
            change.unstaged_code()
        }
        .to_string();
        let display = change.path.clone();
        let dirty = change.untracked();

        let row_view = view.clone();
        let row_change = change.clone();
        let row = ListItem::new(format!("change-{id}"))
            .child(
                div()
                    .w(px(14.))
                    .text_xs()
                    .font_semibold()
                    .text_color(code_color)
                    .child(code_label),
            )
            .child(
                div()
                    .flex_1()
                    .text_sm()
                    .truncate()
                    .child(if dirty {
                        SharedString::from(format!("{display}  (新文件)"))
                    } else {
                        SharedString::from(display.clone())
                    }),
            )
            .on_click(move |_, window, cx| {
                let change = row_change.clone();
                row_view.update(cx, |this, cx| {
                    this.open_change_diff(change.clone(), staged, window, cx)
                });
            });

        let menu_view = view.clone();
        let menu_change = change.clone();
        let menu_change2 = change.clone();
        div().child(row).context_menu(move |menu, _, _| {
            let mut menu = menu.item(
                PopupMenuItem::new(if staged { "取消暂存" } else { "暂存" })
                    .icon(if staged { IconName::Minus } else { IconName::Plus })
                    .on_click({
                        let view = menu_view.clone();
                        let change = menu_change.clone();
                        move |_, _, cx| {
                            view.update(cx, |this, cx| {
                                if staged {
                                    this.on_unstage(change.clone(), cx);
                                } else {
                                    this.on_stage(change.clone(), cx);
                                }
                            })
                        }
                    }),
            );
            if !staged {
                menu = menu.separator();
                menu = menu.item(
                    PopupMenuItem::new("丢弃改动")
                        .icon(IconName::Undo2)
                        .on_click({
                            let view = menu_view.clone();
                            let change = menu_change2.clone();
                            move |_, window, cx| {
                                view.update(cx, |this, cx| {
                                    this.on_discard(change.clone(), window, cx)
                                })
                            }
                        }),
                );
            }
            menu
        })
        .into_any_element()
    }

    // ---------- 渲染：编辑区 ----------

    /// 文档内查找替换栏，打开时占据工具栏下方的一行
    fn render_find_bar(&mut self, cx: &mut Context<Self>) -> Option<AnyElement> {
        if !self.find.open {
            return None;
        }
        let border = cx.theme().border;
        let muted = cx.theme().muted_foreground;
        let view = cx.entity();

        let input = self.find.input.clone();
        let replace_input = self.find.replace.clone();
        let case_sensitive = self.find.case_sensitive;
        let show_replace = self.find.show_replace;
        let total = self.find.matches.len();
        let counter = if total == 0 {
            "无结果".to_string()
        } else {
            format!("{}/{}", self.find.current + 1, total)
        };

        let row = h_flex()
            .w_full()
            .px_2()
            .py_1()
            .gap_1()
            .items_center()
            .border_b_1()
            .border_color(border)
            .child(
                div()
                    .w(px(220.))
                    .flex_shrink_0()
                    .child(Input::new(&input).cleanable(true)),
            )
            .child(
                div()
                    .flex_shrink_0()
                    .min_w(px(56.))
                    .text_xs()
                    .text_color(muted)
                    .child(counter),
            )
            .child(
                Button::new("find-prev")
                    .ghost()
                    .small()
                    .icon(IconName::ChevronUp)
                    .tooltip("上一个（Shift+Enter）")
                    .disabled(total == 0)
                    .on_click({
                        let view = view.clone();
                        move |_, _, cx| view.update(cx, |this, cx| this.find_step(false, cx))
                    }),
            )
            .child(
                Button::new("find-next")
                    .ghost()
                    .small()
                    .icon(IconName::ChevronDown)
                    .tooltip("下一个（Enter）")
                    .disabled(total == 0)
                    .on_click({
                        let view = view.clone();
                        move |_, _, cx| view.update(cx, |this, cx| this.find_step(true, cx))
                    }),
            )
            .child(
                Button::new("find-case")
                    .ghost()
                    .small()
                    .selected(case_sensitive)
                    .icon(IconName::CaseSensitive)
                    .tooltip("区分大小写")
                    .on_click({
                        let view = view.clone();
                        move |_, _, cx| {
                            view.update(cx, |this, cx| {
                                this.find.case_sensitive = !this.find.case_sensitive;
                                this.refresh_find(cx);
                            })
                        }
                    }),
            )
            .child(
                Button::new("find-toggle-replace")
                    .ghost()
                    .small()
                    .selected(show_replace)
                    .icon(IconName::Replace)
                    .tooltip("替换")
                    .on_click({
                        let view = view.clone();
                        move |_, _, cx| {
                            view.update(cx, |this, cx| {
                                this.find.show_replace = !this.find.show_replace;
                                cx.notify();
                            })
                        }
                    }),
            )
            .child(div().flex_1())
            .child(
                Button::new("find-close")
                    .ghost()
                    .small()
                    .icon(IconName::X)
                    .tooltip("关闭")
                    .on_click({
                        let view = view.clone();
                        move |_, window, cx| {
                            view.update(cx, |this, cx| this.close_find(window, cx))
                        }
                    }),
            );

        let replace_row = show_replace.then(|| {
            let view = view.clone();
            h_flex()
                .w_full()
                .px_2()
                .pb_1()
                .gap_1()
                .items_center()
                .border_b_1()
                .border_color(border)
                .child(
                    div()
                        .w(px(220.))
                        .flex_shrink_0()
                        .child(Input::new(&replace_input)),
                )
                .child(
                    Button::new("find-replace-one")
                        .ghost()
                        .small()
                        .label("替换")
                        .disabled(total == 0)
                        .on_click({
                            let view = view.clone();
                            move |_, window, cx| {
                                view.update(cx, |this, cx| this.replace_current(window, cx))
                            }
                        }),
                )
                .child(
                    Button::new("find-replace-all")
                        .ghost()
                        .small()
                        .label("全部替换")
                        .disabled(total == 0)
                        .on_click({
                            let view = view.clone();
                            move |_, window, cx| {
                                view.update(cx, |this, cx| this.replace_all_find(window, cx))
                            }
                        }),
                )
                .into_any_element()
        });

        Some(
            v_flex()
                .w_full()
                .flex_shrink_0()
                .bg(cx.theme().background)
                .child(row)
                .children(replace_row)
                .into_any_element(),
        )
    }

    fn render_main(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> AnyElement {
        let border = cx.theme().border;
        let muted = cx.theme().muted_foreground;
        let view = cx.entity();
        let root = self.root.clone();

        let tab_bg = cx.theme().background;
        let tab_bar_bg = cx.theme().tab_bar;
        let tab_fg = cx.theme().tab_foreground;
        let tab_active_fg = cx.theme().tab_active_foreground;

        // 自绘标签条：4px + 图标 + 4px + 文件名 + 关闭按钮
        let tab_icons = self.icons.clone();
        let tab_strip = h_flex()
            .id("tab-strip")
            .flex_1()
            .min_w_0()
            .h_full()
            .items_center()
            .overflow_hidden()
            .bg(tab_bar_bg)
            .children(self.tabs.iter().enumerate().map(|(ix, tab)| {
                let title = tab.title();
                let dirty = tab.dirty(cx);
                let icon = tab_icons.tab(tab.path());
                let selected = self.active == Some(ix);
                let close_view = view.clone();
                let click_view = view.clone();
                let mut tab_el = h_flex()
                    .id(("tab", ix))
                    .h_full()
                    .flex_shrink_0()
                    .items_center()
                    .gap_1()
                    .pl_1()
                    .pr_1()
                    .border_r_1()
                    .border_color(border)
                    .text_sm()
                    .text_color(if selected { tab_active_fg } else { tab_fg })
                    .cursor_pointer();
                if selected {
                    tab_el = tab_el.bg(tab_bg);
                }
                tab_el
                    .child(icon)
                    .children(dirty.then(|| div().text_xs().child("●")))
                    .child(
                        div()
                            .whitespace_nowrap()
                            .truncate()
                            .max_w(px(220.))
                            .child(title),
                    )
                    .child(
                        Button::new(format!("tab-close-{ix}"))
                            .ghost()
                            .small()
                            .icon(IconName::X)
                            .tooltip("关闭（Ctrl+W）")
                            .on_click(move |_, window, cx| {
                                // 关闭按钮的点击不再冒泡给整条标签，避免误切换
                                cx.stop_propagation();
                                close_view.update(cx, |this, cx| this.request_close(ix, window, cx))
                            }),
                    )
                    .on_click(move |_, window, cx| {
                        click_view.update(cx, |this, cx| this.activate(ix, window, cx))
                    })
                    .into_any_element()
            }));

        let mode = self.active_doc().map(|doc| doc.view).unwrap_or(ViewMode::Edit);
        let dirty = self
            .active
            .and_then(|ix| self.tabs.get(ix))
            .is_some_and(|tab| tab.dirty(cx));
        let is_file = self.active_doc().is_some();

        // 排版工具栏：字体 · 字号 · 背景
        let font_family = self.font_family();
        let font_size = self.font_size;
        let current_font = self.font;
        let current_theme = self.theme_choice;
        let current_width = self.column_width;

        let font_buttons = FontChoice::ALL
            .into_iter()
            .enumerate()
            .map(|(ix, choice)| {
                let view = view.clone();
                Button::new(("font-choice", ix))
                    .ghost()
                    .small()
                    .label(choice.short())
                    .tooltip(choice.label())
                    .selected(current_font == choice)
                    .on_click(move |_, _, cx| {
                        view.update(cx, |this, cx| {
                            this.font = choice;
                            cx.notify();
                        })
                    })
            });

        let size_view = view.clone();
        let size_button = Button::new("font-size")
            .ghost()
            .small()
            .label(format!("{font_size}"))
            .tooltip("字号")
            .dropdown_menu(move |menu, _window, _cx| {
                FONT_SIZES.into_iter().fold(menu, |menu, size| {
                    let view = size_view.clone();
                    menu.item(
                        PopupMenuItem::new(format!("{size}"))
                            .checked((font_size - size).abs() < f32::EPSILON)
                            .on_click(move |_, _, cx| {
                                view.update(cx, |this, cx| {
                                    this.font_size = size;
                                    cx.notify();
                                })
                            }),
                    )
                })
            });

        let theme_buttons = ThemeChoice::ALL
            .into_iter()
            .enumerate()
            .map(|(ix, choice)| {
                let view = view.clone();
                Button::new(("theme-choice", ix))
                    .ghost()
                    .small()
                    .label(choice.label())
                    .tooltip(format!("背景：{}", choice.label()))
                    .selected(current_theme == choice)
                    .on_click(move |_, _, cx| {
                        view.update(cx, |this, cx| this.apply_theme(choice, cx))
                    })
            });

        let width_buttons = ColumnWidth::ALL
            .into_iter()
            .enumerate()
            .map(|(ix, choice)| {
                let view = view.clone();
                Button::new(("column-width", ix))
                    .ghost()
                    .small()
                    .label(choice.label())
                    .tooltip(format!("正文宽度：{}", choice.label()))
                    .selected(current_width == choice)
                    .on_click(move |_, _, cx| {
                        view.update(cx, |this, cx| {
                            this.column_width = choice;
                            cx.notify();
                        })
                    })
            });

        let find_open = self.find.open;
        let find_replace = self.find.open && self.find.show_replace;

        let style_bar = h_flex()
            .w_full()
            .h(px(34.))
            .flex_shrink_0()
            .px_2()
            .gap_0p5()
            .items_center()
            .border_b_1()
            .border_color(border)
            .bg(tab_bg)
            .children(font_buttons)
            .child(div().w(px(1.)).h_4().bg(border).mx_1())
            .child(size_button)
            .child(div().w(px(1.)).h_4().bg(border).mx_1())
            .children(theme_buttons)
            .child(div().w(px(1.)).h_4().bg(border).mx_1())
            .children(width_buttons)
            .child(div().w(px(1.)).h_4().bg(border).mx_1())
            .child(
                Button::new("find-open")
                    .ghost()
                    .small()
                    .label("查找")
                    .tooltip("在本文档里查找（Ctrl+F）")
                    .selected(find_open && !find_replace)
                    .disabled(!is_file)
                    .on_click({
                        let view = view.clone();
                        move |_, window, cx| {
                            view.update(cx, |this, cx| {
                                if this.find.open && this.find.show_replace {
                                    // 替换行还在，就只把它收起来，查找行留着
                                    this.find.show_replace = false;
                                    this.find
                                        .input
                                        .update(cx, |state, cx| state.focus(window, cx));
                                    this.refresh_find(cx);
                                } else if this.find.open {
                                    this.close_find(window, cx);
                                } else {
                                    this.open_find(false, window, cx);
                                }
                            })
                        }
                    }),
            )
            .child(
                Button::new("find-replace")
                    .ghost()
                    .small()
                    .label("替换")
                    .tooltip("在本文档里替换（Ctrl+H）")
                    .selected(find_replace)
                    .disabled(!is_file)
                    .on_click({
                        let view = view.clone();
                        move |_, window, cx| {
                            view.update(cx, |this, cx| {
                                if this.find.open && this.find.show_replace {
                                    this.close_find(window, cx);
                                } else {
                                    this.open_find(true, window, cx);
                                }
                            })
                        }
                    }),
            );

        let toolbar = h_flex()
            .px_2()
            .py_1()
            .gap_0p5()
            .items_center()
            .border_b_1()
            .border_color(border)
            .child(
                Button::new("mode-edit")
                    .ghost()
                    .small()
                    .selected(mode == ViewMode::Edit)
                    .icon(IconName::Pencil)
                    .tooltip("所见即所得")
                    .disabled(!is_file)
                    .on_click({
                        let view = view.clone();
                        move |_, window, cx| {
                            view.update(cx, |this, cx| this.switch_view(ViewMode::Edit, window, cx))
                        }
                    }),
            )
            .child(
                Button::new("mode-source")
                    .ghost()
                    .small()
                    .selected(mode == ViewMode::Source)
                    .icon(IconName::Code)
                    .tooltip("源码")
                    .disabled(!is_file)
                    .on_click({
                        let view = view.clone();
                        move |_, window, cx| {
                            view.update(cx, |this, cx| {
                                this.switch_view(ViewMode::Source, window, cx)
                            })
                        }
                    }),
            )
            .child(
                Button::new("mode-preview")
                    .ghost()
                    .small()
                    .selected(mode == ViewMode::Preview)
                    .icon(IconName::Eye)
                    .tooltip("演示（Ctrl+Shift+V 轮换）")
                    .disabled(!is_file)
                    .on_click({
                        let view = view.clone();
                        move |_, window, cx| {
                            view.update(cx, |this, cx| {
                                this.switch_view(ViewMode::Preview, window, cx)
                            })
                        }
                    }),
            )
            .child(
                Button::new("mode-split")
                    .ghost()
                    .small()
                    .selected(mode == ViewMode::Split)
                    .icon(IconName::SquareSplitHorizontal)
                    .tooltip("对照")
                    .disabled(!is_file)
                    .on_click({
                        let view = view.clone();
                        move |_, window, cx| {
                            view.update(cx, |this, cx| this.switch_view(ViewMode::Split, window, cx))
                        }
                    }),
            )
            .child(div().flex_1())
            .child(
                Button::new("save-doc")
                    .ghost()
                    .small()
                    .icon(IconName::Save)
                    .tooltip("保存（Ctrl+S）")
                    .disabled(!dirty)
                    .on_click({
                        let view = view.clone();
                        move |_, _, cx| {
                            view.update(cx, |this, cx| this.save_active(cx));
                        }
                    }),
            );

        // 演示视图在渲染前跟上文档的最新文本：在所见即所得里改过字之后，
        // 切到演示或做查找高亮才对得上
        if let Some(ix) = self.active
            && let Some(Tab::File(doc)) = self.tabs.get_mut(ix)
            && matches!(doc.view, ViewMode::Preview | ViewMode::Split)
        {
            let src = preview_source(&doc.text(cx));
            if doc.preview_src != src {
                doc.preview_src = src.clone();
                doc.preview.update(cx, |state, cx| state.set_text(&src, cx));
            }
        }

        let content_width = px(self.column_width.max_width());
        let body: AnyElement = match self.active.and_then(|ix| self.tabs.get(ix)) {
            Some(Tab::File(doc)) => {
                let editor = doc.editor.clone();
                let wysiwyg = doc.wysiwyg.clone();
                let wys_scroll = doc.wys_scroll.clone();
                let preview = doc.preview.clone();
                let mode = doc.view;
                match mode {
                    // 所见即所得：直接按 Markdown 渲染，点哪里就在哪里编辑
                    ViewMode::Edit => div()
                        .id("wysiwyg-scroll")
                        .flex_1()
                        .min_h_0()
                        .w_full()
                        .overflow_y_scroll()
                        .track_scroll(&wys_scroll)
                        .px_8()
                        .py_6()
                        .child(
                            div()
                                .w_full()
                                .max_w(content_width)
                                .mx_auto()
                                .font_family(font_family.clone())
                                .text_size(px(font_size))
                                .child(BlockEditor::new(&wysiwyg).min_h_full()),
                        )
                        .into_any_element(),
                    // 源码：只显示 Markdown 原文，适合精修格式
                    ViewMode::Source => div()
                        .flex_1()
                        .h_full()
                        .min_h_0()
                        .w_full()
                        .overflow_hidden()
                        .px_6()
                        .py_4()
                        .child(
                            Editor::new(&editor)
                                .h_full()
                                .font_family(font_family.clone())
                                .text_size(px(font_size)),
                        )
                        .into_any_element(),
                    ViewMode::Split => h_flex()
                        .flex_1()
                        .h_full()
                        .min_h_0()
                        .w_full()
                        .items_stretch()
                        .on_drag_move(cx.listener(
                            |this, event: &DragMoveEvent<SplitDrag>, _, cx| {
                                let width = f32::from(event.bounds.size.width);
                                if width <= 0. {
                                    return;
                                }
                                let x = f32::from(event.event.position.x - event.bounds.origin.x);
                                let ratio = (x / width).clamp(MIN_SPLIT, MAX_SPLIT);
                                if (this.split_ratio - ratio).abs() > 0.001 {
                                    this.split_ratio = ratio;
                                    cx.notify();
                                }
                            },
                        ))
                        .child(
                            div()
                                .flex_basis(relative(self.split_ratio))
                                .flex_shrink_0()
                                .min_w_0()
                                .h_full()
                                .overflow_hidden()
                                .px_3()
                                .py_2()
                                .child(
                                    Editor::new(&editor)
                                        .h_full()
                                        .font_family(font_family.clone())
                                        .text_size(px(font_size)),
                                ),
                        )
                        .child(
                            div()
                                .id("split-divider")
                                .w(px(5.))
                                .h_full()
                                .flex_shrink_0()
                                .bg(border)
                                .cursor(CursorStyle::ResizeLeftRight)
                                .on_drag(SplitDrag, |_, _, _, cx| cx.new(|_| SplitGhost)),
                        )
                        .child(
                            div()
                                .id("preview-split-scroll")
                                .flex_1()
                                .min_w_0()
                                .h_full()
                                .overflow_y_scrollbar()
                                .px_6()
                                .py_5()
                                .child(
                                    div()
                                        .w_full()
                                        .max_w(content_width)
                                        .mx_auto()
                                        .child(
                                            TextView::new(&preview).selectable(true),
                                        ),
                                ),
                        )
                        .into_any_element(),
                    ViewMode::Preview => div()
                        .id("preview-only-scroll")
                        .flex_1()
                        .min_h_0()
                        .w_full()
                        .overflow_y_scrollbar()
                        .px_8()
                        .py_6()
                        .child(
                            div()
                                .w_full()
                                .max_w(content_width)
                                .mx_auto()
                                .child(TextView::new(&preview).selectable(true)),
                        )
                        .into_any_element(),
                }
            }
            Some(Tab::Diff { state, .. }) => {
                let state = state.clone();
                // Diff 内部用虚拟列表按视口高度渲染，必须给它一个确定高度的容器，
                // 否则行数为 0，整个区域会是空白
                div()
                    .flex_1()
                    .h_full()
                    .min_h_0()
                    .w_full()
                    .overflow_hidden()
                    .child(Diff::new(&state).h_full())
                    .into_any_element()
            }
            None => v_flex()
                .flex_1()
                .min_h_0()
                .w_full()
                .items_center()
                .justify_center()
                .gap_3()
                .child(Icon::new(IconName::NotebookPen).size_8().text_color(muted))
                .child(
                    div()
                        .text_sm()
                        .text_color(muted)
                        .child("从左侧选择一篇笔记，或新建一篇"),
                )
                .child(
                    Button::new("new-note-empty")
                        .primary()
                        .small()
                        .label("新建笔记")
                        .on_click({
                            let view = view.clone();
                            let root = root.clone();
                            move |_, window, cx| {
                                view.update(cx, |this, cx| {
                                    this.start_inline(InlineKind::NewFile, root.clone(), window, cx)
                                })
                            }
                        }),
                )
                .into_any_element(),
        };

        let find_bar = self.render_find_bar(cx);

        v_flex()
            .flex_1()
            .min_w_0()
            .h_full()
            .child(style_bar)
            .child(
                h_flex()
                    .w_full()
                    .h(px(34.))
                    .items_center()
                    .border_b_1()
                    .border_color(border)
                    .child(tab_strip)
                    .child(toolbar),
            )
            .children(find_bar)
            .child(v_flex().flex_1().min_h_0().w_full().child(body))
            .into_any_element()
    }

    // ---------- 渲染：状态栏 ----------

    fn render_status_bar(&self, cx: &mut Context<Self>) -> AnyElement {
        let border = cx.theme().border;
        let muted = cx.theme().muted_foreground;

        let detail = match self.active.and_then(|ix| self.tabs.get(ix)) {
            Some(Tab::File(doc)) => {
                let text = doc.text(cx);
                let chars = text.chars().filter(|c| !c.is_whitespace()).count();
                let lines = text.lines().count().max(1);
                let dirty = if doc.is_dirty(cx) { " · 未保存" } else { "" };
                format!("字数 {chars} · 行 {lines}{dirty}")
            }
            Some(Tab::Diff { .. }) => "差异视图".to_string(),
            None => String::new(),
        };

        h_flex()
            .h(px(24.))
            .flex_shrink_0()
            .px_3()
            .gap_3()
            .items_center()
            .border_t_1()
            .border_color(border)
            .text_xs()
            .text_color(muted)
            .child(
                div()
                    .flex_1()
                    .min_w_0()
                    .truncate()
                    .child(self.root.to_string_lossy().to_string()),
            )
            .children((!self.status.is_empty()).then(|| {
                div()
                    .max_w(px(380.))
                    .truncate()
                    .child(self.status.clone())
            }))
            .child(div().whitespace_nowrap().child(detail))
            .into_any_element()
    }
}

impl Render for NotesApp {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        // 切换工作区后新生成的欢迎文档在这里打开（创建编辑器需要 Window）
        if let Some(path) = self.pending_open.take() {
            self.open_file(path, window, cx);
        }

        let background = cx.theme().background;
        let foreground = cx.theme().foreground;
        let border = cx.theme().border;

        v_flex()
            .size_full()
            .bg(background)
            .text_color(foreground)
            .key_context("NotesApp")
            .on_action(cx.listener(Self::on_save))
            .on_action(cx.listener(Self::on_close_tab))
            .on_action(cx.listener(Self::on_next_tab))
            .on_action(cx.listener(Self::on_prev_tab))
            .on_action(cx.listener(Self::on_toggle_preview))
            .on_action(cx.listener(Self::on_rename_selected))
            .on_action(cx.listener(Self::on_delete_selected))
            .on_action(cx.listener(Self::on_refresh))
            .on_action(cx.listener(Self::on_focus_search))
            .on_action(cx.listener(Self::on_focus_find))
            .on_action(cx.listener(Self::on_open_replace))
            .child(
                // TitleBar 默认带 12px 左内边距，这里压到一个字符宽，图标与文字之间也留一个字符宽
                TitleBar::new().pl_2().child(
                    h_flex()
                        .w_full()
                        .pr_3()
                        .gap_2()
                        .items_center()
                        .child(Icon::new(IconName::NotebookPen).size_4())
                        .child(div().text_sm().font_semibold().child("笔记本")),
                ),
            )
            .child(
                h_flex()
                    .flex_1()
                    .min_h_0()
                    .w_full()
                    .on_drag_move(cx.listener(
                        |this, event: &DragMoveEvent<SidebarDrag>, _, cx| {
                            let x = f32::from(event.event.position.x - event.bounds.origin.x);
                            let width = x.clamp(MIN_SIDEBAR, MAX_SIDEBAR);
                            if (this.sidebar_width - width).abs() > 0.5 {
                                this.sidebar_width = width;
                                cx.notify();
                            }
                        },
                    ))
                    .child(self.render_sidebar(cx))
                    .child(
                        div()
                            .id("sidebar-divider")
                            .w(px(4.))
                            .h_full()
                            .flex_shrink_0()
                            .bg(border)
                            .cursor(CursorStyle::ResizeLeftRight)
                            .on_drag(SidebarDrag, |_, _, _, cx| cx.new(|_| SplitGhost)),
                    )
                    .child(self.render_main(window, cx)),
            )
            .child(self.render_status_bar(cx))
    }
}

/// 取输出的第一行，避免把多行 git 输出塞进状态栏
fn first_line(text: &str) -> String {
    text.lines()
        .find(|line| !line.trim().is_empty())
        .unwrap_or("")
        .trim()
        .to_string()
}

/// 预览前把段落内的换行变成硬换行，让「回车」在渲染结果里也换行。
///
/// CommonMark 只认行尾两个空格或反斜杠为硬换行，段落里的单个换行会被折叠成
/// 空格。这里给需要保留的换行补上两个空格，代码块、表格和空行保持原样。
fn preview_source(text: &str) -> String {
    let mut out = String::with_capacity(text.len() + text.len() / 8);
    let lines: Vec<&str> = text.lines().collect();
    let mut fence: Option<&'static str> = None;

    for (ix, line) in lines.iter().enumerate() {
        let trimmed = line.trim_start();
        // 围栏代码块内部原样保留，只跟踪开合
        if let Some(marker) = fence {
            if trimmed.starts_with(marker) {
                fence = None;
            }
            out.push_str(line);
            out.push('\n');
            continue;
        }
        if let Some(marker) = ["```", "~~~"].into_iter().find(|m| trimmed.starts_with(m)) {
            fence = Some(marker);
            out.push_str(line);
            out.push('\n');
            continue;
        }

        // 下一行还有内容才算段落内的换行，空行已经能分隔段落
        let continues = lines
            .get(ix + 1)
            .is_some_and(|next| !next.trim().is_empty());
        // 表格行、分隔线、HTML 块这类结构行不加行尾空格，免得影响块解析
        let structural = trimmed.contains('|')
            || trimmed.starts_with('<')
            || line
                .trim()
                .chars()
                .all(|c| matches!(c, '-' | '=' | '_' | '*'));
        // 反斜杠本身就是硬换行，不用再补空格
        let needs_break =
            continues && !line.trim().is_empty() && !structural && !line.ends_with('\\');

        out.push_str(line);
        if needs_break {
            out.push_str("  ");
        }
        out.push('\n');
    }

    // lines() 会吞掉结尾的换行，原文不是以换行结尾时把多加的那个去掉
    if !text.ends_with('\n') {
        out.pop();
    }
    out
}

/// 内嵌的阅读字体：编译期直接打包进可执行文件，不依赖外部 ttf
static WENKAI_TTF: &[u8] = include_bytes!("../assets/fonts/LXGWWenKai-Medium.ttf");
static PUHUITI_TTF: &[u8] = include_bytes!("../assets/fonts/AlibabaPuHuiTi-3-65-Medium.ttf");

/// 注册内嵌的霞鹜文楷 / 阿里巴巴普惠体，并返回实际可用的字体族名
fn load_reading_fonts(cx: &App) -> (SharedString, SharedString) {
    cx.text_system()
        .add_fonts(vec![Cow::Borrowed(WENKAI_TTF), Cow::Borrowed(PUHUITI_TTF)])
        .ok();

    let families = cx.text_system().all_font_names();
    let resolve = |needles: &[&str], fallback: &str| -> SharedString {
        families
            .iter()
            .find(|name| {
                let lower = name.to_lowercase();
                needles.iter().any(|needle| lower.contains(needle))
            })
            .map(|name| name.to_string().into())
            .unwrap_or_else(|| fallback.into())
    };

    (
        resolve(&["wenkai", "wen kai", "霞鹜"], "LXGW WenKai"),
        resolve(&["puhuiti", "pu hui ti", "普惠"], "Alibaba PuHuiTi 3.0"),
    )
}
