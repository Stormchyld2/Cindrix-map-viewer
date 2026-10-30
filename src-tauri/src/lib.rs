//! CindrixOps Map Viewer: opens the phone map file (`.cmap`) made by the main CindrixOps program and shows it
//! with no network. The file is only ever read.
pub mod cmap;

use serde::Serialize;
use std::sync::{Arc, Mutex};

/// The map that is open now (one at a time).
static OPEN: Mutex<Option<Arc<cmap::OpenCmap>>> = Mutex::new(None);

fn current() -> Option<Arc<cmap::OpenCmap>> {
    OPEN.lock().ok().and_then(|g| g.clone())
}

#[derive(Serialize)]
struct Info {
    #[serde(flatten)]
    manifest: cmap::Manifest,
    /// Kinds of picture the file carries but this viewer cannot draw yet.
    unsupported: Vec<String>,
}

#[tauri::command]
fn cmap_open(path: String) -> Result<Info, String> {
    let c = cmap::OpenCmap::open(std::path::Path::new(&path))?;
    let info = Info { manifest: c.manifest.clone(), unsupported: c.unsupported() };
    *OPEN.lock().map_err(|_| "Could not open the map.".to_string())? = Some(Arc::new(c));
    Ok(info)
}

/// A `.cmap` file given on the command line (a double-click on Windows), if any.
#[tauri::command]
fn startup_file() -> Option<String> {
    std::env::args().skip(1).find(|a| a.to_lowercase().ends_with(".cmap") && std::path::Path::new(a).is_file())
}

fn reply(status: u16, content_type: Option<&str>, body: Vec<u8>) -> tauri::http::Response<Vec<u8>> {
    let mut b = tauri::http::Response::builder().status(status).header("Access-Control-Allow-Origin", "*");
    if let Some(t) = content_type {
        b = b.header("Content-Type", t);
    }
    b.body(body).unwrap_or_else(|_| tauri::http::Response::new(Vec::new()))
}

fn percent_decode(s: &str) -> String {
    let b = s.as_bytes();
    let mut out = Vec::with_capacity(b.len());
    let mut i = 0;
    while i < b.len() {
        if b[i] == b'%' && i + 2 < b.len() {
            let hex = std::str::from_utf8(&b[i + 1..i + 3]).ok().and_then(|h| u8::from_str_radix(h, 16).ok());
            if let Some(v) = hex {
                out.push(v);
                i += 3;
                continue;
            }
        }
        out.push(b[i]);
        i += 1;
    }
    String::from_utf8_lossy(&out).into_owned()
}

/// Answer one request for a tile (`/tiles/<kind>/<z>/<x>/<y>`) or lettering (`/glyphs/<font>/<range>.pbf`).
fn serve(path: &str) -> tauri::http::Response<Vec<u8>> {
    let Some(map) = current() else { return reply(404, None, Vec::new()) };
    let parts: Vec<&str> = path.trim_start_matches('/').split('/').collect();
    let found = match parts.as_slice() {
        ["tiles", kind, z, x, y] => {
            let y = y.split('.').next().unwrap_or("");
            match (z.parse::<u32>(), x.parse::<u32>(), y.parse::<u32>()) {
                (Ok(z), Ok(x), Ok(y)) => map.tile(kind, z, x, y),
                _ => return reply(400, None, Vec::new()),
            }
        }
        ["glyphs", font, range] => {
            let font = percent_decode(font);
            let first = font.split(',').next().unwrap_or("").trim().to_string();
            let range = range.trim_end_matches(".pbf");
            map.glyph(&first, range)
        }
        _ => return reply(404, None, Vec::new()),
    };
    match found {
        Ok(Some(p)) => reply(200, Some(p.content_type), p.bytes),
        // A tile or lettering file the map does not hold is 404: the map engine treats that as an empty square
        // (outside the area, or beyond the detail the map was cut with), not as a fault.
        Ok(None) => reply(404, None, Vec::new()),
        Err(_) => reply(500, None, Vec::new()),
    }
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_dialog::init())
        .register_asynchronous_uri_scheme_protocol("cmap", |_ctx, request, responder| {
            let path = request.uri().path().to_string();
            std::thread::spawn(move || responder.respond(serve(&path)));
        })
        .invoke_handler(tauri::generate_handler![cmap_open, startup_file])
        .setup(|_app| {
            #[cfg(desktop)]
            {
                use tauri::Manager;
                if let Some(window) = _app.get_webview_window("main") {
                    let _ = window.set_fullscreen(true);
                }
            }
            Ok(())
        })
        .run(tauri::generate_context!())
        .expect("error running CindrixOps Map Viewer");
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn percent_decoding_handles_spaces_and_stray_signs() {
        assert_eq!(percent_decode("Noto%20Sans%20Regular"), "Noto Sans Regular");
        assert_eq!(percent_decode("100%"), "100%");
        assert_eq!(percent_decode("a%2"), "a%2");
        assert_eq!(percent_decode("plain"), "plain");
    }

    #[test]
    fn requests_with_no_map_open_or_a_bad_path_get_plain_answers() {
        assert_eq!(serve("/tiles/street/1/2/3").status(), 404);
        assert_eq!(serve("/nothing").status(), 404);
    }
}
