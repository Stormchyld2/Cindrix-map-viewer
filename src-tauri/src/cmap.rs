//! CMAP v1 offline map package reader/writer. ZIP is a transport container, not encryption.
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::{collections::{BTreeMap, HashSet}, fs::File, io::{Read, Write}, path::Path};
use zip::{write::SimpleFileOptions, CompressionMethod, ZipArchive, ZipWriter};

const MAX_FILES: usize = 10_000;
const MAX_ENTRY: u64 = 8 * 1024 * 1024 * 1024;
const MAX_TOTAL: u64 = 32 * 1024 * 1024 * 1024;
const MAX_MANIFEST: u64 = 4 * 1024 * 1024;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Entry {
    pub path: String,
    pub bytes: u64,
    pub sha256: String,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct MapAssets {
    pub tiles: String,
    pub style: String,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Manifest {
    pub format: String,
    pub format_version: u32,
    pub package_id: String,
    pub title: String,
    pub created_utc: String,
    pub bounds_wgs84: [f64; 4],
    pub map: MapAssets,
    #[serde(default)]
    pub overlays: Vec<String>,
    pub entries: Vec<Entry>,
    pub attribution: String,
    pub minimum_viewer_version: String,
}
fn fail(s: impl Into<String>) -> String { s.into() }
fn valid_path(p: &str) -> bool {
    !p.is_empty() && p.len() <= 512 && !p.starts_with('/') && !p.contains('\\')
        && !p.contains(':') && !p.chars().any(char::is_control)
        && p.split('/').all(|x| !x.is_empty() && x != "." && x != "..")
}
fn check_manifest(m: &Manifest) -> Result<(), String> {
    if m.format != "cindrix-cmap" || m.format_version != 1 { return Err(fail("Unsupported CMAP format/version")); }
    if m.package_id.trim().is_empty() || m.title.trim().is_empty() || m.created_utc.trim().is_empty()
        || m.minimum_viewer_version.trim().is_empty() { return Err(fail("Missing required metadata")); }
    let [w,s,e,n] = m.bounds_wgs84;
    if ![w,s,e,n].iter().all(|v| v.is_finite()) || w < -180. || e > 180. || s < -90. || n > 90. || w >= e || s >= n {
        return Err(fail("Invalid WGS84 bounds"));
    }
    if m.entries.is_empty() || m.entries.len() > MAX_FILES { return Err(fail("Invalid entry count")); }
    let mut paths = HashSet::new();
    let mut total = 0u64;
    for ent in &m.entries {
        if !valid_path(&ent.path) || ent.path == "manifest.json" || !paths.insert(ent.path.as_str()) {
            return Err(fail("Invalid or duplicate payload path"));
        }
        if ent.bytes > MAX_ENTRY { return Err(fail("Entry exceeds limit")); }
        total = total.checked_add(ent.bytes).ok_or("Size overflow")?;
        if total > MAX_TOTAL { return Err(fail("Package exceeds size limit")); }
        if ent.sha256.len() != 64 || !ent.sha256.bytes().all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b)) {
            return Err(fail("Invalid SHA-256"));
        }
    }
    for required in [&m.map.tiles, &m.map.style, &m.attribution] {
        if !paths.contains(required.as_str()) { return Err(fail("Required asset missing from manifest")); }
    }
    for overlay in &m.overlays {
        if !paths.contains(overlay.as_str()) { return Err(fail("Overlay missing from manifest")); }
    }
    if !m.map.tiles.ends_with(".pmtiles") || !m.map.style.ends_with(".json") {
        return Err(fail("Invalid map asset types"));
    }
    Ok(())
}
fn digest(data: &[u8]) -> String { format!("{:x}", Sha256::digest(data)) }
/// Validates archive structure, declared sizes and SHA-256 digests; does not install or extract files.
pub fn inspect(path: &Path) -> Result<Manifest, String> {
    let file = File::open(path).map_err(|e| e.to_string())?;
    let mut zip = ZipArchive::new(file).map_err(|e| e.to_string())?;
    if zip.len() < 2 || zip.len() > MAX_FILES + 1 { return Err(fail("Invalid archive entry count")); }
    let mut seen = HashSet::new();
    let mut manifest_data = None;
    let mut total = 0u64;
    for i in 0..zip.len() {
        let mut entry = zip.by_index(i).map_err(|e| e.to_string())?;
        let name = entry.name().to_string();
        if !valid_path(&name) || !seen.insert(name.clone()) || entry.is_dir() {
            return Err(fail("Unsafe, duplicate, or directory archive entry"));
        }
        if entry.unix_mode().map(|mode| mode & 0o170000 == 0o120000).unwrap_or(false) {
            return Err(fail("Symlinks not allowed"));
        }
        if entry.size() > MAX_ENTRY { return Err(fail("Oversized archive entry")); }
        total = total.checked_add(entry.size()).ok_or("Archive size overflow")?;
        if total > MAX_TOTAL { return Err(fail("Archive exceeds size limit")); }
        if name == "manifest.json" {
            if entry.size() > MAX_MANIFEST { return Err(fail("Manifest too large")); }
            let mut data = Vec::new();
            entry.read_to_end(&mut data).map_err(|e| e.to_string())?;
            manifest_data = Some(data);
        }
    }
    let data = manifest_data.ok_or("Missing manifest.json")?;
    let m: Manifest = serde_json::from_slice(&data).map_err(|e| e.to_string())?;
    check_manifest(&m)?;
    if m.entries.len() + 1 != seen.len() { return Err(fail("Undeclared archive entry")); }
    for ent in &m.entries {
        if !seen.contains(&ent.path) { return Err(fail("Declared file missing")); }
        let mut f = zip.by_name(&ent.path).map_err(|e| e.to_string())?;
        if f.size() != ent.bytes { return Err(fail("Size mismatch")); }
        let mut hasher = Sha256::new();
        let mut buf = [0u8; 65536];
        let mut read = 0u64;
        loop {
            let n = f.read(&mut buf).map_err(|e| e.to_string())?;
            if n == 0 { break; }
            read += n as u64;
            if read > ent.bytes { return Err(fail("Excess payload data")); }
            hasher.update(&buf[..n]);
        }
        if read != ent.bytes || format!("{:x}", hasher.finalize()) != ent.sha256 {
            return Err(fail(format!("Integrity check failed: {}", ent.path)));
        }
    }
    Ok(m)
}
/// Writes a self-contained package from already-validated relative payload names and bytes.
pub fn write_package(path: &Path, mut manifest: Manifest, files: &BTreeMap<String, Vec<u8>>) -> Result<(), String> {
    manifest.entries = files.iter().map(|(name, data)| Entry {
        path: name.clone(), bytes: data.len() as u64, sha256: digest(data),
    }).collect();
    check_manifest(&manifest)?;
    let temp = path.with_extension("cmap.tmp");
    let result = (|| -> Result<(), String> {
        let f = File::create(&temp).map_err(|e| e.to_string())?;
        let mut writer = ZipWriter::new(f);
        let options = SimpleFileOptions::default().compression_method(CompressionMethod::Stored);
        writer.start_file("manifest.json", options).map_err(|e| e.to_string())?;
        writer.write_all(&serde_json::to_vec_pretty(&manifest).map_err(|e| e.to_string())?).map_err(|e| e.to_string())?;
        for (name, data) in files {
            writer.start_file(name, options).map_err(|e| e.to_string())?;
            writer.write_all(data).map_err(|e| e.to_string())?;
        }
        writer.finish().map_err(|e| e.to_string())?;
        inspect(&temp)?;
        if path.exists() { return Err(fail("Destination already exists; refusing overwrite")); }
        std::fs::rename(&temp, path).map_err(|e| e.to_string())?;
        Ok(())
    })();
    if result.is_err() { let _ = std::fs::remove_file(&temp); }
    result
}
#[cfg(test)]
mod tests {
    use super::*;
    fn fixture() -> (Manifest, BTreeMap<String, Vec<u8>>) {
        let files = BTreeMap::from([
            ("map/region.pmtiles".to_string(), b"PMTiles fixture".to_vec()),
            ("map/style.json".to_string(), b"{\"version\":8}".to_vec()),
            ("metadata/attribution.json".to_string(), b"{}".to_vec()),
        ]);
        let m = Manifest { format: "cindrix-cmap".into(), format_version: 1, package_id: "test-id".into(),
            title: "Test".into(), created_utc: "2026-10-22T00:00:00Z".into(),
            bounds_wgs84: [-80.,38.,-79.,39.], map: MapAssets { tiles:"map/region.pmtiles".into(),style:"map/style.json".into() },
            overlays: vec![], entries: vec![], attribution:"metadata/attribution.json".into(),
            minimum_viewer_version:"1.0.0".into() };
        (m, files)
    }
    #[test] fn round_trip() {
        let dir = tempfile::tempdir().unwrap(); let path = dir.path().join("region.cmap");
        let (m,files)=fixture(); write_package(&path,m,&files).unwrap();
        assert_eq!(inspect(&path).unwrap().entries.len(),3);
    }
    #[test] fn rejects_traversal() {
        let (mut m, mut files)=fixture();
        files.insert("../escape".into(), vec![1]);
        m.entries=files.iter().map(|(p,d)|Entry{path:p.clone(),bytes:d.len() as u64,sha256:digest(d)}).collect();
        assert!(check_manifest(&m).is_err());
    }
    #[test] fn rejects_duplicate_manifest_entries() {
        let (mut m,files)=fixture();
        m.entries=files.iter().map(|(p,d)|Entry{path:p.clone(),bytes:d.len() as u64,sha256:digest(d)}).collect();
        m.entries.push(m.entries[0].clone());
        assert!(check_manifest(&m).is_err());
    }
    #[test] fn rejects_unsupported_version() {
        let (mut m,files)=fixture();
        m.entries=files.iter().map(|(p,d)|Entry{path:p.clone(),bytes:d.len() as u64,sha256:digest(d)}).collect();
        m.format_version=2; assert!(check_manifest(&m).is_err());
    }
    #[test] fn rejects_modified_payload() {
        let dir=tempfile::tempdir().unwrap(); let path=dir.path().join("tampered.cmap");
        let (m,files)=fixture(); write_package(&path,m,&files).unwrap();
        let bytes=std::fs::read(&path).unwrap();
        let mut corrupted=bytes.clone();
        let pos=corrupted.windows(15).position(|w|w==b"PMTiles fixture").unwrap();
        corrupted[pos]=b'X'; std::fs::write(&path,corrupted).unwrap();
        assert!(inspect(&path).is_err());
    }
    #[test] fn refuses_overwrite() {
        let dir=tempfile::tempdir().unwrap();let path=dir.path().join("map.cmap");
        let (m,files)=fixture();write_package(&path,m.clone(),&files).unwrap();
        assert!(write_package(&path,m,&files).is_err());
    }
}
