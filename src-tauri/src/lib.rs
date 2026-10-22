#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .setup(|app| {
            use tauri::Manager;
            if let Some(window) = app.get_webview_window("main") {
                window.set_fullscreen(true)?;
            }
            Ok(())
        })
        .run(tauri::generate_context!())
        .expect("error running CindrixOps CMAP Viewer");
}
