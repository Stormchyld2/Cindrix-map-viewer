pub mod cmap;
mod fsio;
mod maps;

use serde::Serialize;
use std::path::PathBuf;

fn data_root() -> PathBuf {
    let base = std::env::var_os("CINDRIXOPS_VIEWER_DATA")
        .map(PathBuf::from)
        .unwrap_or_else(|| {
            #[cfg(target_os = "windows")]
            { std::env::var_os("APPDATA").map(PathBuf::from).unwrap_or_else(std::env::temp_dir).join("CindrixOps").join("CMAP Viewer") }
            #[cfg(not(target_os = "windows"))]
            { std::env::var_os("HOME").map(PathBuf::from).unwrap_or_else(std::env::temp_dir).join(".cindrixops-cmap-viewer") }
        });
    base
}
fn store() -> maps::MapStore { maps::MapStore::new(&data_root().join("user")) }
#[derive(Serialize)]
struct MapMetaDto { id: String, name: String, has_picture: bool, has_route: bool }
#[derive(Serialize)]
struct MapDto { json: String, notes: String }
#[derive(Serialize)]
struct RouteStats { km: f64, miles: f64 }
#[derive(Serialize)]
struct PackDto { rel: String, name: String, kind: String, mb: u64 }
#[derive(Serialize)]
struct PlaceResults { places: Vec<PlaceDto>, has_list: bool }
#[derive(Serialize)]
struct PlaceDto { name: String, state: String, kind: String, lat: f64, lon: f64 }

#[tauri::command]
fn maps_list() -> Vec<MapMetaDto> {
    store().list().into_iter().map(|m| MapMetaDto { id: m.id, name: m.name, has_picture: m.has_picture, has_route: m.has_route }).collect()
}
#[tauri::command]
fn map_load(id: String) -> Result<MapDto, String> {
    let (json, notes) = store().load(&id)?;
    Ok(MapDto { json, notes })
}
#[tauri::command]
fn map_save(id: Option<String>, name: String, json: String, notes: String) -> Result<String, String> {
    store().save(id.as_deref(), &name, &json, &notes)
}
#[tauri::command]
fn map_delete(id: String) -> Result<(), String> { store().delete(&id) }
#[tauri::command]
fn map_export_gpx(id: String, name: String, points: Vec<(f64, f64)>) -> Result<String, String> {
    store().export_gpx(&id, &name, &points).map(|p| p.to_string_lossy().to_string())
}
#[tauri::command]
fn route_stats(points: Vec<(f64, f64)>) -> RouteStats {
    let km = maps::route_km(&points);
    RouteStats { km, miles: km * 0.621_371_192 }
}
#[tauri::command]
fn map_packs() -> Vec<PackDto> {
    maps::packs(&data_root().join("maps")).into_iter().map(|p| PackDto { rel: p.rel, name: p.name, kind: p.kind, mb: p.mb }).collect()
}
#[tauri::command]
fn place_search(_query: String) -> PlaceResults {
    PlaceResults { places: Vec::new(), has_list: false }
}
fn tile_reply(status: u16, content_range: Option<String>, body: Vec<u8>) -> tauri::http::Response<Vec<u8>> {
    let mut b = tauri::http::Response::builder().status(status)
        .header("Access-Control-Allow-Origin", "*")
        .header("Access-Control-Allow-Methods", "GET, OPTIONS")
        .header("Access-Control-Allow-Headers", "Range")
        .header("Access-Control-Expose-Headers", "Content-Range, Accept-Ranges, Content-Length")
        .header("Accept-Ranges", "bytes");
    if let Some(r) = content_range { b = b.header("Content-Range", r); }
    if !body.is_empty() { b = b.header("Content-Type", "application/octet-stream"); }
    b.body(body).unwrap_or_else(|_| tauri::http::Response::new(Vec::new()))
}
fn serve_tile(req: &tauri::http::Request<Vec<u8>>) -> tauri::http::Response<Vec<u8>> {
    if *req.method() == tauri::http::Method::OPTIONS { return tile_reply(204, None, Vec::new()); }
    if *req.method() != tauri::http::Method::GET { return tile_reply(405, None, Vec::new()); }
    let root = data_root().join("maps");
    if let Some(font) = maps::resolve_font_path(&root, req.uri().path()) {
        return match std::fs::read(font) {
            Ok(bytes) => tauri::http::Response::builder().status(200)
                .header("Access-Control-Allow-Origin", "*")
                .header("Content-Type", "application/x-protobuf")
                .body(bytes).unwrap_or_else(|_| tauri::http::Response::new(Vec::new())),
            Err(_) => tile_reply(404, None, Vec::new()),
        };
    }
    let Some(path) = maps::resolve_tile_path(&root, req.uri().path()) else { return tile_reply(404, None, Vec::new()); };
    let Ok(len) = std::fs::metadata(&path).map(|m| m.len()) else { return tile_reply(404, None, Vec::new()); };
    let Some(range) = req.headers().get("range").and_then(|v| v.to_str().ok()) else {
        return tile_reply(416, Some(format!("bytes */{len}")), Vec::new());
    };
    match maps::parse_range(range, len) {
        Ok((start, end)) => match maps::read_range(&path, start, end) {
            Ok(bytes) => tile_reply(206, Some(format!("bytes {start}-{end}/{len}")), bytes),
            Err(_) => tile_reply(500, None, Vec::new()),
        },
        Err(maps::RangeError::Unsatisfiable) => tile_reply(416, Some(format!("bytes */{len}")), Vec::new()),
        Err(maps::RangeError::Unsupported) => tile_reply(400, None, Vec::new()),
    }
}
#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .register_asynchronous_uri_scheme_protocol("cindrix-tiles", |_ctx, request, responder| {
            std::thread::spawn(move || responder.respond(serve_tile(&request)));
        })
        .invoke_handler(tauri::generate_handler![
            maps_list, map_load, map_save, map_delete, map_export_gpx,
            route_stats, map_packs, place_search
        ])
        .setup(|app| {
            use tauri::Manager;
            if let Some(window) = app.get_webview_window("main") { window.set_fullscreen(true)?; }
            Ok(())
        })
        .run(tauri::generate_context!())
        .expect("error running CindrixOps CMAP Viewer");
}
