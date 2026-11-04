fn main() {
    // 把 assets/icon.ico 写进可执行文件资源，任务栏与快捷方式都会用到它
    println!("cargo:rerun-if-changed=assets/icon.ico");
    println!("cargo:rerun-if-changed=assets/app.rc");
    let _ = embed_resource::compile("assets/app.rc", embed_resource::NONE);
}
