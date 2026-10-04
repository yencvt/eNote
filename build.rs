// Embeds `assets/icon.ico` as the Windows executable's resource icon, so the exe shows
// the "e" logo everywhere Explorer displays it - the taskbar, Alt+Tab, the file's own
// icon, and (since the "Open with eNote" context menu entry references this same exe via
// `"{exe}",0`, see `shell_integration.rs`) the right-click menu too. A no-op on other
// targets, where Windows PE resources don't apply.
fn main() {
    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() == Ok("windows") {
        let mut res = winresource::WindowsResource::new();
        res.set_icon("assets/icon.ico");
        if let Err(e) = res.compile() {
            println!("cargo:warning=failed to embed Windows icon resource: {e}");
        }
    }
}
