// 笔记本：一个 VSCode 风格的 Markdown 笔记本
// 笔记根目录：运行目录下的 notes/（不存在会自动创建）
//
// 界面：
//   - 左侧栏：资源管理器 / 源代码管理（Git）两个视图
//   - 资源管理器：目录树，右键可新建 / 重命名 / 删除，可切换工作区
//   - 编辑区：多标签，默认所见即所得，可切到「编辑 + 演示」或「演示」
//   - 状态栏：提示信息 / 当前文档字数 / 行数 / 保存状态
//
// 快捷键：
//   Ctrl+S           保存当前标签
//   Ctrl+W           关闭当前标签
//   Ctrl+Tab         下一个标签
//   Ctrl+Shift+Tab   上一个标签
//   Ctrl+Shift+V     切换 编辑 / 编辑 + 演示 / 演示
//   F2               重命名目录树中选中的文件
//   Delete           删除目录树中选中的文件（焦点在目录树上时）
//   F5               刷新目录树与 Git 状态

// 发布构建不附带控制台窗口；调试构建保留控制台方便看日志
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod app;
mod file_icons;
mod fs_ops;
mod git;

use std::path::PathBuf;

use app::NotesApp;

use gpui_kit::*;

actions!(
    notebook,
    [
        Save,
        CloseTab,
        NextTab,
        PrevTab,
        TogglePreview,
        RenameSelected,
        DeleteSelected,
        Refresh,
    ]
);

const NOTES_DIR: &str = "notes";

fn main() {
    // 优先回到上次的工作区；目录已不存在时退回运行目录下的 notes/
    let default_root = std::env::current_dir()
        .unwrap_or_else(|_| PathBuf::from("."))
        .join(NOTES_DIR);
    let root = fs_ops::load_session()
        .and_then(|session| session.root)
        .filter(|root| root.is_dir())
        .unwrap_or(default_root);
    let _ = std::fs::create_dir_all(&root);

    application()
        .with_assets(assets::AllAssets)
        .run(move |cx| {
            component::init(cx);
            markdown_editor::init(cx);
            cx.bind_keys([
                KeyBinding::new("ctrl-s", Save, Some("NotesApp")),
                KeyBinding::new("ctrl-w", CloseTab, Some("NotesApp")),
                KeyBinding::new("ctrl-tab", NextTab, Some("NotesApp")),
                KeyBinding::new("ctrl-shift-tab", PrevTab, Some("NotesApp")),
                KeyBinding::new("ctrl-shift-v", TogglePreview, Some("NotesApp")),
                KeyBinding::new("f5", Refresh, Some("NotesApp")),
                // 只有焦点在目录树上时才生效，避免误删编辑器内容
                KeyBinding::new("f2", RenameSelected, Some("Tree")),
                KeyBinding::new("delete", DeleteSelected, Some("Tree")),
            ]);

            // 默认窗口 1320×880，由 gpui 按显示器工作区居中并收敛
            let bounds = Bounds::centered(None, size(px(1320.), px(880.)), cx);
            let options = WindowOptions {
                window_bounds: Some(WindowBounds::Windowed(bounds)),
                titlebar: Some(TitlebarOptions {
                    appears_transparent: true,
                    title: Some("笔记本".into()),
                    ..Default::default()
                }),
                ..Default::default()
            };

            // 必须用 gpui_kit::open_window 开窗：它会把内容包进 Root，
            // 否则对话框（open_dialog）找不到窗口状态会直接 panic
            open_window(options, cx, move |window, cx| {
                cx.new(|cx| NotesApp::new(root.clone(), window, cx))
            })
            .expect("打开窗口失败");
        });
}
