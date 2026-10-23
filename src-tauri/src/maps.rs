//! Saved maps (doc 2): one folder per map under `user/saved-maps/<id>/` holding
//! `map.json` (pins, rendezvous points, routes, no-go areas, labels, written by the interface),
//! `notes.md`, an optional `picture.png` and an optional `route.gpx`. Also the distance maths and
//! the GPX writer. Standard library only. Nothing here touches the internet.

use crate::fsio;
use std::fs;
use std::path::{Path, PathBuf};

/// Earth radius in kilometres (mean).
const EARTH_KM: f64 = 6371.0088;

/// Great-circle distance between two (latitude, longitude) points in degrees, in kilometres.
pub fn haversine_km(a: (f64, f64), b: (f64, f64)) -> f64 {
    let (la1, lo1, la2, lo2) = (a.0.to_radians(), a.1.to_radians(), b.0.to_radians(), b.1.to_radians());
    let h = ((la2 - la1) / 2.0).sin().powi(2) + la1.cos() * la2.cos() * ((lo2 - lo1) / 2.0).sin().powi(2);
    2.0 * EARTH_KM * h.sqrt().asin()
}

/// Length of a route through `points`, in kilometres.
pub fn route_km(points: &[(f64, f64)]) -> f64 {
    points.windows(2).map(|w| haversine_km(w[0], w[1])).sum()
}

/// Initial compass bearing from `a` to `b`, 0 to 360 degrees (0 is north).
pub fn bearing_deg(a: (f64, f64), b: (f64, f64)) -> f64 {
    let (la1, la2, dlo) = (a.0.to_radians(), b.0.to_radians(), (b.1 - a.1).to_radians());
    let y = dlo.sin() * la2.cos();
    let x = la1.cos() * la2.sin() - la1.sin() * la2.cos() * dlo.cos();
    (y.atan2(x).to_degrees() + 360.0) % 360.0
}

/// A point is valid when it is a real latitude and longitude.
pub fn valid_point(p: (f64, f64)) -> bool {
    p.0.is_finite() && p.1.is_finite() && (-90.0..=90.0).contains(&p.0) && (-180.0..=180.0).contains(&p.1)
}

/// Is `p` inside the polygon `poly` (a no-go area)? Planar test on degrees: fine for areas
/// of a few kilometres, which is what a flooded road or a blocked bridge is.
pub fn in_polygon(p: (f64, f64), poly: &[(f64, f64)]) -> bool {
    if poly.len() < 3 {
        return false;
    }
    let mut inside = false;
    let mut j = poly.len() - 1;
    for i in 0..poly.len() {
        let (yi, xi) = poly[i];
        let (yj, xj) = poly[j];
        if (yi > p.0) != (yj > p.0) && p.1 < (xj - xi) * (p.0 - yi) / (yj - yi) + xi {
            inside = !inside;
        }
        j = i;
    }
    inside
}

fn xml_escape(s: &str) -> String {
    s.replace('&', "&amp;").replace('<', "&lt;").replace('>', "&gt;").replace('"', "&quot;")
}

/// A GPX 1.1 file with one route, so phone map apps can open it. Points outside real
/// latitude/longitude ranges are an error rather than a file that fails later.
pub fn gpx(name: &str, points: &[(f64, f64)]) -> Result<String, String> {
    if points.len() < 2 {
        return Err("A route needs at least two points.".into());
    }
    if let Some(bad) = points.iter().find(|p| !valid_point(**p)) {
        return Err(format!("Not a real position: {}, {}.", bad.0, bad.1));
    }
    let mut s = String::from("<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n<gpx version=\"1.1\" creator=\"Cindrix Ops\" xmlns=\"http://www.topografix.com/GPX/1/1\">\n");
    s.push_str(&format!("  <rte>\n    <name>{}</name>\n", xml_escape(name)));
    for (lat, lon) in points {
        s.push_str(&format!("    <rtept lat=\"{:.6}\" lon=\"{:.6}\"/>\n", lat, lon));
    }
    s.push_str("  </rte>\n</gpx>\n");
    Ok(s)
}

/// Pull a string value out of the (flat, top-level) `"name"` field of a map file.
fn name_of(json: &str) -> String {
    let Some(i) = json.find("\"name\"") else { return String::new() };
    let rest = &json[i + 6..];
    let Some(c) = rest.find(':') else { return String::new() };
    let Some(rest) = rest[c + 1..].trim_start().strip_prefix('"') else { return String::new() };
    let mut out = String::new();
    let mut esc = false;
    for ch in rest.chars() {
        if esc {
            out.push(ch);
            esc = false;
        } else if ch == '\\' {
            esc = true;
        } else if ch == '"' {
            break;
        } else {
            out.push(ch);
        }
    }
    out
}

/// A map file must be a JSON object that says which version wrote it (doc 2: every JSON file
/// is versioned). This is a guard against saving garbage, not a full JSON parser.
pub fn looks_like_map_json(json: &str) -> bool {
    let t = json.trim();
    t.starts_with('{') && t.ends_with('}') && t.contains("\"version\"")
}

/// Folder-safe form of a name: lowercase letters, digits and single hyphens.
pub fn slug(name: &str) -> String {
    let mut out = String::new();
    for ch in name.to_lowercase().chars() {
        if ch.is_ascii_alphanumeric() {
            out.push(ch);
        } else if !out.is_empty() && !out.ends_with('-') {
            out.push('-');
        }
    }
    let out = out.trim_end_matches('-').to_string();
    let out: String = out.chars().take(48).collect();
    if out.is_empty() { "map".to_string() } else { out }
}

/// An id is only ever one of our own slugs, so it can never reach outside the saved-maps folder.
pub fn valid_id(id: &str) -> bool {
    !id.is_empty() && id.len() <= 60 && id.chars().all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-')
}

#[derive(Debug, Clone, PartialEq)]
pub struct MapMeta {
    pub id: String,
    pub name: String,
    pub has_picture: bool,
    pub has_route: bool,
}

pub struct MapStore {
    root: PathBuf,
    backups: PathBuf,
}

impl MapStore {
    /// `user` is the drive's `user/` folder.
    pub fn new(user: &Path) -> MapStore {
        MapStore { root: user.join("saved-maps"), backups: user.join("backups") }
    }

    fn dir(&self, id: &str) -> Result<PathBuf, String> {
        if valid_id(id) { Ok(self.root.join(id)) } else { Err("That is not a valid map name.".into()) }
    }

    /// Every saved map, by name. A folder without a readable map.json is skipped, not an error.
    pub fn list(&self) -> Vec<MapMeta> {
        let Ok(rd) = fs::read_dir(&self.root) else { return vec![] };
        let mut v: Vec<MapMeta> = rd
            .flatten()
            .filter(|e| e.path().is_dir())
            .filter_map(|e| {
                let id = e.file_name().to_string_lossy().to_string();
                if !valid_id(&id) {
                    return None;
                }
                let text = fs::read_to_string(e.path().join("map.json")).ok()?;
                let name = name_of(&text);
                Some(MapMeta {
                    name: if name.is_empty() { id.clone() } else { name },
                    has_picture: e.path().join("picture.png").is_file(),
                    has_route: e.path().join("route.gpx").is_file(),
                    id,
                })
            })
            .collect();
        v.sort_by_key(|a| a.name.to_lowercase());
        v
    }

    /// The map file and its notes. A missing notes file is empty notes.
    pub fn load(&self, id: &str) -> Result<(String, String), String> {
        let d = self.dir(id)?;
        let json = fs::read_to_string(d.join("map.json")).map_err(|e| format!("Could not read the map: {}", e))?;
        let notes = fs::read_to_string(d.join("notes.md")).unwrap_or_default();
        Ok((json, notes))
    }

    /// Save a map. `id` None makes a new folder named from `name`; an existing id is replaced,
    /// with the old map.json copied to `backups/` first. Returns the id.
    pub fn save(&self, id: Option<&str>, name: &str, json: &str, notes: &str) -> Result<String, String> {
        if !looks_like_map_json(json) {
            return Err("That is not a saved map (missing version).".into());
        }
        let id = match id {
            Some(i) => {
                self.dir(i)?;
                i.to_string()
            }
            None => {
                let base = slug(name);
                let mut cand = base.clone();
                let mut n = 2;
                while self.root.join(&cand).exists() {
                    cand = format!("{}-{}", base, n);
                    n += 1;
                }
                cand
            }
        };
        let d = self.dir(&id)?;
        fsio::backup_then_write(&d.join("map.json"), &self.backups, json.as_bytes()).map_err(|e| e.to_string())?;
        fsio::backup_then_write(&d.join("notes.md"), &self.backups, notes.as_bytes()).map_err(|e| e.to_string())?;
        Ok(id)
    }

    /// Write the exported picture. Only a real PNG is accepted.
    pub fn save_picture(&self, id: &str, png: &[u8]) -> Result<(), String> {
        if png.len() < 8 || png[..8] != [0x89, b'P', b'N', b'G', 0x0d, 0x0a, 0x1a, 0x0a] {
            return Err("That is not a PNG picture.".into());
        }
        let d = self.dir(id)?;
        if !d.join("map.json").is_file() {
            return Err("Save the map before adding a picture.".into());
        }
        fsio::write_atomic(&d.join("picture.png"), png).map_err(|e| e.to_string())
    }

    /// Write `route.gpx` for a saved map from its route points.
    pub fn export_gpx(&self, id: &str, name: &str, points: &[(f64, f64)]) -> Result<PathBuf, String> {
        let d = self.dir(id)?;
        if !d.join("map.json").is_file() {
            return Err("Save the map before exporting its route.".into());
        }
        let text = gpx(name, points)?;
        let p = d.join("route.gpx");
        fsio::write_atomic(&p, text.as_bytes()).map_err(|e| e.to_string())?;
        Ok(p)
    }

    /// Remove a saved map. The folder is copied nowhere: this is the person's explicit choice.
    pub fn delete(&self, id: &str) -> Result<(), String> {
        let d = self.dir(id)?;
        if !d.join("map.json").is_file() {
            return Err("There is no such saved map.".into());
        }
        fs::remove_dir_all(d).map_err(|e| e.to_string())
    }
}

// ---------- map packs (PMTiles files) and serving them by byte range ----------
//
// A map pack is one `.pmtiles` file under the drive's `maps/` folder (doc 2: `maps/us/street/`,
// `maps/us/topo/`, `maps/satellite/<state>/<county>/`). The interface's map library reads a
// PMTiles file with HTTP Range requests, so the shell only has to hand out byte ranges of the
// file. These functions decide which files may be handed out and read the bytes.

use std::io::{Read, Seek, SeekFrom};

/// Most bytes returned for one range request. PMTiles asks for small pieces (a 16 KB header
/// and directory, then single tiles), so a larger request is cut short rather than trusted.
pub const MAX_RANGE_BYTES: u64 = 32 * 1024 * 1024;

#[derive(Debug, Clone, PartialEq)]
pub struct Pack {
    /// Path under `maps/`, with `/` separators, for example `us/street/us.pmtiles`.
    pub rel: String,
    /// File name without the extension.
    pub name: String,
    /// "street", "topo", "satellite" or "other", from the folder it sits in.
    pub kind: String,
    pub mb: u64,
}

fn pack_kind(rel: &str) -> &'static str {
    let r = rel.to_lowercase();
    if r.contains("street") {
        "street"
    } else if r.contains("topo") {
        "topo"
    } else if r.contains("satellite") {
        "satellite"
    } else {
        "other"
    }
}

/// Every `.pmtiles` file under `maps_root`, up to four folders deep, sorted by path.
pub fn packs(maps_root: &Path) -> Vec<Pack> {
    let mut out = Vec::new();
    let mut stack = vec![(maps_root.to_path_buf(), 0u8)];
    while let Some((dir, depth)) = stack.pop() {
        let Ok(rd) = fs::read_dir(&dir) else { continue };
        for e in rd.flatten() {
            let Ok(ft) = e.file_type() else { continue };
            let p = e.path();
            if ft.is_dir() {
                if depth < 4 {
                    stack.push((p, depth + 1));
                }
            } else if ft.is_file() && p.extension().map(|x| x.to_string_lossy().eq_ignore_ascii_case("pmtiles")).unwrap_or(false) {
                let Ok(rel) = p.strip_prefix(maps_root) else { continue };
                let rel = rel.components().map(|c| c.as_os_str().to_string_lossy().to_string()).collect::<Vec<_>>().join("/");
                out.push(Pack {
                    name: p.file_stem().map(|n| n.to_string_lossy().to_string()).unwrap_or_default(),
                    kind: pack_kind(&rel).to_string(),
                    mb: e.metadata().map(|m| m.len() / 1_048_576).unwrap_or(0),
                    rel,
                });
            }
        }
    }
    out.sort_by(|a, b| a.rel.cmp(&b.rel));
    out
}

pub(crate) fn percent_decode(s: &str) -> Option<String> {
    let b = s.as_bytes();
    let mut out = Vec::with_capacity(b.len());
    let mut i = 0;
    while i < b.len() {
        if b[i] == b'%' {
            let h = s.get(i + 1..i + 3)?;
            out.push(u8::from_str_radix(h, 16).ok()?);
            i += 3;
        } else {
            out.push(b[i]);
            i += 1;
        }
    }
    String::from_utf8(out).ok()
}

/// Turn the path of a tile request into a real file, but only a `.pmtiles` file that sits inside
/// `maps_root`. Anything else (parent folders, absolute paths, other file types, links that lead
/// outside the folder) gives None. This is the only door from the web view to the disk.
pub fn resolve_tile_path(maps_root: &Path, requested: &str) -> Option<PathBuf> {
    let path = requested.split(['?', '#']).next().unwrap_or("");
    let decoded = percent_decode(path)?;
    let rel = decoded.trim_start_matches('/');
    if rel.is_empty() || rel.contains('\\') || rel.contains('\0') {
        return None;
    }
    let mut full = maps_root.to_path_buf();
    for part in rel.split('/') {
        let ok = !part.is_empty()
            && part != "."
            && part != ".."
            && part.chars().all(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '_' | '-' | ' '));
        if !ok {
            return None;
        }
        full.push(part);
    }
    if !full.extension().map(|x| x.to_string_lossy().eq_ignore_ascii_case("pmtiles")).unwrap_or(false) {
        return None;
    }
    let real = full.canonicalize().ok()?;
    let root = maps_root.canonicalize().ok()?;
    if real.starts_with(&root) && real.is_file() { Some(real) } else { None }
}

/// The map's lettering comes as small glyph files: `fonts/<font name>/<first>-<last>.pbf` under the
/// maps folder. Only exactly that shape is handed out, and only from inside `maps_root/fonts`.
/// The web view encodes the font name once more than the file name needs, so the path is decoded
/// up to twice.
pub fn resolve_font_path(maps_root: &Path, requested: &str) -> Option<PathBuf> {
    let path = requested.split(['?', '#']).next().unwrap_or("");
    let mut decoded = percent_decode(path)?;
    if decoded.contains('%') {
        decoded = percent_decode(&decoded)?;
    }
    let parts: Vec<&str> = decoded.trim_start_matches('/').split('/').collect();
    if parts.len() != 3 || parts[0] != "fonts" {
        return None;
    }
    let (stack, file) = (parts[1], parts[2]);
    if stack.is_empty() || stack.len() > 60 || !stack.chars().all(|c| c.is_ascii_alphanumeric() || matches!(c, ' ' | '-' | '_')) {
        return None;
    }
    let range = file.strip_suffix(".pbf")?;
    let (a, b) = range.split_once('-')?;
    if a.is_empty() || b.is_empty() || a.len() > 7 || b.len() > 7 || !a.chars().all(|c| c.is_ascii_digit()) || !b.chars().all(|c| c.is_ascii_digit()) {
        return None;
    }
    let full = maps_root.join("fonts").join(stack).join(file);
    let real = full.canonicalize().ok()?;
    let root = maps_root.join("fonts").canonicalize().ok()?;
    if real.starts_with(&root) && real.is_file() && fs::metadata(&real).map(|m| m.len() <= 2 * 1024 * 1024).unwrap_or(false) {
        Some(real)
    } else {
        None
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RangeError {
    /// The header is not a single `bytes=` range we understand.
    Unsupported,
    /// The range starts past the end of the file.
    Unsatisfiable,
}

/// Read an HTTP `Range` header against a file of `len` bytes. Returns inclusive (start, end),
/// with `end` cut to the end of the file and to MAX_RANGE_BYTES.
pub fn parse_range(header: &str, len: u64) -> Result<(u64, u64), RangeError> {
    let spec = header.trim().strip_prefix("bytes=").ok_or(RangeError::Unsupported)?;
    if spec.contains(',') || len == 0 {
        return Err(if len == 0 { RangeError::Unsatisfiable } else { RangeError::Unsupported });
    }
    let (a, b) = spec.split_once('-').ok_or(RangeError::Unsupported)?;
    let (start, end) = if a.trim().is_empty() {
        // "-N": the last N bytes
        let n: u64 = b.trim().parse().map_err(|_| RangeError::Unsupported)?;
        if n == 0 {
            return Err(RangeError::Unsatisfiable);
        }
        (len.saturating_sub(n), len - 1)
    } else {
        let s: u64 = a.trim().parse().map_err(|_| RangeError::Unsupported)?;
        let e: Option<u64> = if b.trim().is_empty() { None } else { Some(b.trim().parse().map_err(|_| RangeError::Unsupported)?) };
        if matches!(e, Some(e) if e < s) {
            return Err(RangeError::Unsupported);
        }
        if s >= len {
            return Err(RangeError::Unsatisfiable);
        }
        (s, e.unwrap_or(len - 1).min(len - 1))
    };
    if start >= len {
        return Err(RangeError::Unsatisfiable);
    }
    Ok((start, end.min(start + MAX_RANGE_BYTES - 1)))
}

/// Read bytes `start..=end` of a file.
pub fn read_range(path: &Path, start: u64, end: u64) -> std::io::Result<Vec<u8>> {
    let mut f = fs::File::open(path)?;
    f.seek(SeekFrom::Start(start))?;
    let mut buf = vec![0u8; (end - start + 1) as usize];
    f.read_exact(&mut buf)?;
    Ok(buf)
}

/// Does this look like a PMTiles v3 file? The file starts with the seven letters PMTiles and
/// the version byte 3.
pub fn is_pmtiles_v3(head: &[u8]) -> bool {
    head.len() >= 8 && &head[..7] == b"PMTiles" && head[7] == 3
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tmp(name: &str) -> PathBuf {
        let p = std::env::temp_dir().join(format!("cindrix-maps-{}-{}", name, std::process::id()));
        let _ = fs::remove_dir_all(&p);
        fs::create_dir_all(&p).unwrap();
        p
    }

    const MAP: &str = r#"{ "version": 1, "name": "Home to \"Camp\"", "pins": [] }"#;

    #[test]
    fn distances_are_right() {
        // One degree of latitude is about 111.2 km.
        let d = haversine_km((0.0, 0.0), (1.0, 0.0));
        assert!((d - 111.195).abs() < 0.1, "{}", d);
        // Equator, one degree of longitude is the same.
        assert!((haversine_km((0.0, 0.0), (0.0, 1.0)) - 111.195).abs() < 0.1);
        // New York to Los Angeles is about 3936 km.
        let ny_la = haversine_km((40.7128, -74.0060), (34.0522, -118.2437));
        assert!((ny_la - 3936.0).abs() < 15.0, "{}", ny_la);
        assert_eq!(haversine_km((10.0, 10.0), (10.0, 10.0)), 0.0);
    }

    #[test]
    fn route_length_adds_the_legs() {
        let pts = [(0.0, 0.0), (1.0, 0.0), (1.0, 1.0)];
        let want = haversine_km(pts[0], pts[1]) + haversine_km(pts[1], pts[2]);
        assert!((route_km(&pts) - want).abs() < 1e-9);
        assert_eq!(route_km(&[(1.0, 1.0)]), 0.0);
        assert_eq!(route_km(&[]), 0.0);
    }

    #[test]
    fn bearings_point_the_right_way() {
        assert!((bearing_deg((0.0, 0.0), (1.0, 0.0)) - 0.0).abs() < 1e-6); // north
        assert!((bearing_deg((0.0, 0.0), (0.0, 1.0)) - 90.0).abs() < 1e-6); // east
        assert!((bearing_deg((0.0, 0.0), (-1.0, 0.0)) - 180.0).abs() < 1e-6); // south
        assert!((bearing_deg((0.0, 0.0), (0.0, -1.0)) - 270.0).abs() < 1e-6); // west
    }

    #[test]
    fn no_go_polygon_test() {
        let square = [(0.0, 0.0), (0.0, 1.0), (1.0, 1.0), (1.0, 0.0)];
        assert!(in_polygon((0.5, 0.5), &square));
        assert!(!in_polygon((1.5, 0.5), &square));
        assert!(!in_polygon((0.5, -0.1), &square));
        assert!(!in_polygon((0.5, 0.5), &square[..2])); // not an area
    }

    #[test]
    fn positions_are_checked() {
        assert!(valid_point((39.0, -80.0)));
        assert!(!valid_point((91.0, 0.0)));
        assert!(!valid_point((0.0, 181.0)));
        assert!(!valid_point((f64::NAN, 0.0)));
    }

    #[test]
    fn gpx_is_well_formed_and_escaped() {
        let g = gpx("Home & <Camp>", &[(39.1, -80.2), (39.2, -80.3)]).unwrap();
        assert!(g.starts_with("<?xml"));
        assert!(g.contains("<name>Home &amp; &lt;Camp&gt;</name>"));
        assert!(g.contains("<rtept lat=\"39.100000\" lon=\"-80.200000\"/>"));
        assert_eq!(g.matches("<rtept").count(), 2);
        assert!(g.trim_end().ends_with("</gpx>"));
        assert!(gpx("x", &[(1.0, 1.0)]).is_err());
        assert!(gpx("x", &[(1.0, 1.0), (95.0, 1.0)]).is_err());
    }

    #[test]
    fn slugs_and_ids_cannot_escape_the_folder() {
        assert_eq!(slug("Home to Camp!"), "home-to-camp");
        assert_eq!(slug("  --  "), "map");
        assert_eq!(slug("../../etc/passwd"), "etc-passwd");
        assert!(valid_id("home-to-camp"));
        for bad in ["", "..", "../x", "a/b", "a\\b", "Home", "a b", "x.json"] {
            assert!(!valid_id(bad), "{:?}", bad);
        }
        let s = MapStore::new(&tmp("escape"));
        assert!(s.load("../x").is_err());
        assert!(s.delete("..").is_err());
        assert!(s.save(Some("../x"), "n", MAP, "").is_err());
    }

    #[test]
    fn save_list_load_round_trip() {
        let user = tmp("rt");
        let s = MapStore::new(&user);
        assert!(s.list().is_empty());
        let id = s.save(None, "Home to Camp", MAP, "Take the back road.").unwrap();
        assert_eq!(id, "home-to-camp");
        let l = s.list();
        assert_eq!(l.len(), 1);
        assert_eq!(l[0].name, "Home to \"Camp\"");
        assert!(!l[0].has_picture && !l[0].has_route);
        let (json, notes) = s.load(&id).unwrap();
        assert_eq!(json, MAP);
        assert_eq!(notes, "Take the back road.");
        // The same name makes a second map, not an overwrite.
        let id2 = s.save(None, "Home to Camp", MAP, "").unwrap();
        assert_eq!(id2, "home-to-camp-2");
        assert_eq!(s.list().len(), 2);
    }

    #[test]
    fn replacing_a_map_keeps_a_backup() {
        let user = tmp("bak");
        let s = MapStore::new(&user);
        let id = s.save(None, "A", MAP, "one").unwrap();
        let newer = r#"{ "version": 1, "name": "A2" }"#;
        s.save(Some(&id), "A", newer, "two").unwrap();
        assert_eq!(s.load(&id).unwrap(), (newer.to_string(), "two".to_string()));
        let n = fs::read_dir(user.join("backups")).unwrap().count();
        assert!(n >= 2, "backups: {}", n); // map.json and notes.md
    }

    #[test]
    fn garbage_is_not_saved_as_a_map() {
        let s = MapStore::new(&tmp("garbage"));
        assert!(s.save(None, "x", "hello", "").is_err());
        assert!(s.save(None, "x", "{ \"name\": \"no version\" }", "").is_err());
        assert!(s.list().is_empty());
    }

    #[test]
    fn picture_and_route_need_a_saved_map_and_real_data() {
        let user = tmp("files");
        let s = MapStore::new(&user);
        let png = [0x89, b'P', b'N', b'G', 0x0d, 0x0a, 0x1a, 0x0a, 0, 0];
        assert!(s.save_picture("nope", &png).is_err());
        let id = s.save(None, "M", MAP, "").unwrap();
        assert!(s.save_picture(&id, b"GIF89a....").is_err());
        s.save_picture(&id, &png).unwrap();
        assert!(s.export_gpx(&id, "M", &[(1.0, 1.0)]).is_err());
        let p = s.export_gpx(&id, "M", &[(39.0, -80.0), (39.1, -80.1)]).unwrap();
        assert!(p.ends_with("route.gpx"));
        let m = &s.list()[0];
        assert!(m.has_picture && m.has_route);
    }

    #[test]
    fn delete_removes_only_that_map() {
        let s = MapStore::new(&tmp("del"));
        let a = s.save(None, "A", MAP, "").unwrap();
        let b = s.save(None, "B", MAP, "").unwrap();
        s.delete(&a).unwrap();
        assert_eq!(s.list().iter().map(|m| m.id.clone()).collect::<Vec<_>>(), vec![b]);
        assert!(s.delete(&a).is_err());
    }

    #[test]
    fn a_damaged_folder_is_skipped_in_the_list() {
        let user = tmp("skip");
        let s = MapStore::new(&user);
        s.save(None, "Good", MAP, "").unwrap();
        fs::create_dir_all(user.join("saved-maps/empty-folder")).unwrap();
        assert_eq!(s.list().len(), 1);
    }

    // ---------- packs and range reads ----------

    fn pack_root(name: &str) -> PathBuf {
        let root = tmp(name).join("maps");
        fs::create_dir_all(root.join("us/street")).unwrap();
        fs::create_dir_all(root.join("us/topo")).unwrap();
        fs::write(root.join("us/street/us.pmtiles"), b"PMTiles\x03 street data 0123456789").unwrap();
        fs::write(root.join("us/topo/contours.PMTILES"), b"PMTiles\x03 topo").unwrap();
        fs::write(root.join("us/street/readme.txt"), b"not a pack").unwrap();
        root
    }

    #[test]
    fn packs_are_found_with_their_kind() {
        let root = pack_root("packs");
        let p = packs(&root);
        assert_eq!(p.len(), 2, "{:?}", p);
        assert_eq!(p[0].rel, "us/street/us.pmtiles");
        assert_eq!(p[0].kind, "street");
        assert_eq!(p[0].name, "us");
        assert_eq!(p[1].kind, "topo");
        assert!(packs(&root.join("nothing-here")).is_empty());
    }

    #[test]
    fn only_pmtiles_inside_the_folder_can_be_served() {
        let root = pack_root("resolve");
        assert!(resolve_tile_path(&root, "/us/street/us.pmtiles").is_some());
        assert!(resolve_tile_path(&root, "us/street/us.pmtiles?x=1").is_some());
        assert!(resolve_tile_path(&root, "/us/topo/contours.PMTILES").is_some());
        // The interface may send the folder separators percent-encoded (Tauri's convertFileSrc).
        assert!(resolve_tile_path(&root, "/us%2Fstreet%2Fus.pmtiles").is_some());
        for bad in [
            "",
            "/",
            "/us/street/readme.txt",
            "/us/street/missing.pmtiles",
            "/../maps/us/street/us.pmtiles",
            "/us/../us/street/us.pmtiles",
            "/us/street/%2e%2e/street/us.pmtiles",
            "//etc/passwd",
            "/us\\street\\us.pmtiles",
            "/us/street/us.pmtiles%00.txt",
            "/us/street",
            "/us/street/%zz.pmtiles",
        ] {
            assert!(resolve_tile_path(&root, bad).is_none(), "should refuse {:?}", bad);
        }
    }

    #[cfg(unix)]
    #[test]
    fn a_link_that_leaves_the_folder_is_refused() {
        let root = pack_root("symlink");
        let outside = root.parent().unwrap().join("secret.pmtiles");
        fs::write(&outside, b"PMTiles\x03 secret").unwrap();
        std::os::unix::fs::symlink(&outside, root.join("us/street/link.pmtiles")).unwrap();
        assert!(resolve_tile_path(&root, "/us/street/link.pmtiles").is_none());
    }

    #[test]
    fn range_headers_are_read_the_standard_way() {
        assert_eq!(parse_range("bytes=0-99", 1000), Ok((0, 99)));
        assert_eq!(parse_range("bytes=900-", 1000), Ok((900, 999)));
        assert_eq!(parse_range("bytes=900-5000", 1000), Ok((900, 999))); // cut to the file
        assert_eq!(parse_range("bytes=-100", 1000), Ok((900, 999)));
        assert_eq!(parse_range("bytes=-5000", 1000), Ok((0, 999)));
        assert_eq!(parse_range("bytes=1000-", 1000), Err(RangeError::Unsatisfiable));
        assert_eq!(parse_range("bytes=-0", 1000), Err(RangeError::Unsatisfiable));
        assert_eq!(parse_range("bytes=0-0", 0), Err(RangeError::Unsatisfiable));
        for bad in ["", "0-99", "bytes=", "bytes=a-b", "bytes=5-2", "bytes=0-1,5-9", "items=0-1"] {
            assert_eq!(parse_range(bad, 1000), Err(RangeError::Unsupported), "{:?}", bad);
        }
    }

    #[test]
    fn a_huge_range_is_cut_short() {
        let big = 10 * MAX_RANGE_BYTES;
        let (s, e) = parse_range("bytes=0-", big).unwrap();
        assert_eq!((s, e - s + 1), (0, MAX_RANGE_BYTES));
    }

    #[test]
    fn ranges_read_the_right_bytes() {
        let root = pack_root("read");
        let p = resolve_tile_path(&root, "/us/street/us.pmtiles").unwrap();
        let len = fs::metadata(&p).unwrap().len();
        let (s, e) = parse_range("bytes=0-7", len).unwrap();
        let head = read_range(&p, s, e).unwrap();
        assert!(is_pmtiles_v3(&head));
        let (s, e) = parse_range("bytes=-10", len).unwrap();
        assert_eq!(read_range(&p, s, e).unwrap(), b"0123456789");
        assert!(!is_pmtiles_v3(b"PMTiles\x02"));
        assert!(!is_pmtiles_v3(b"short"));
    }
    #[test]
    fn font_files_are_handed_out_only_in_their_exact_shape() {
        let root = tmp("fonts");
        let d = root.join("fonts").join("Noto Sans Regular");
        fs::create_dir_all(&d).unwrap();
        fs::write(d.join("0-255.pbf"), b"glyphs").unwrap();
        fs::write(root.join("secret.pbf"), b"x").unwrap();
        fs::write(root.join("fonts").join("notes.txt"), b"x").unwrap();
        let want = d.join("0-255.pbf").canonicalize().unwrap();
        assert_eq!(resolve_font_path(&root, "/fonts/Noto%20Sans%20Regular/0-255.pbf"), Some(want.clone()));
        // encoded once more, the way the web view sends it
        assert_eq!(resolve_font_path(&root, "/fonts%2FNoto%2520Sans%2520Regular%2F0-255.pbf"), Some(want));
        for bad in ["", "/", "/fonts/Noto Sans Regular/0-255.txt", "/fonts/../secret.pbf", "/fonts/Noto Sans Regular/../../secret.pbf", "/fonts/Missing/0-255.pbf",
                    "/fonts/Noto Sans Regular/a-b.pbf", "/other/Noto Sans Regular/0-255.pbf", "/fonts/Noto Sans Regular/0-255.pbf/x", "/fonts/Noto%5CSans/0-255.pbf", "/fonts/notes.txt"] {
            assert_eq!(resolve_font_path(&root, bad), None, "{}", bad);
        }
    }
}
