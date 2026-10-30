//! Reader for the phone map file (`.cmap`) that the main CindrixOps program writes.
//!
//! The format is the one in `Cindrix-Ops/docs/32-phone-map.md` and `core/src/phonepack.rs`:
//!
//! ```text
//! "CMAP0001" | tile data and glyph files | tile lists | manifest (JSON) | manifest length (u32 LE) | "CMAPEND1"
//! ```
//!
//! Each tile list entry is five little-endian u32: `z, x, y, offset, length`. The tile bytes are at
//! `data_offset + offset` in the file, exactly as they were in the source map file (still compressed as the
//! layer says). This reader only reads; it never writes or changes a map file.
use serde::{Deserialize, Serialize};
use std::{
    fs::File,
    io::{Read, Seek, SeekFrom},
    path::{Path, PathBuf},
};

pub const MAGIC: &[u8; 8] = b"CMAP0001";
pub const END_MAGIC: &[u8; 8] = b"CMAPEND1";
/// A map's own data (pins, drawings, directions) is small; anything larger is not a real map file.
const MAX_MANIFEST: u64 = 64 * 1024 * 1024;
/// The most tiles a single layer may list (a 20-byte entry each is read into memory).
const MAX_TILES: u64 = 40_000_000;
/// The most one lettering file may hold.
const MAX_GLYPH: u64 = 4 * 1024 * 1024;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct View {
    /// [latitude, longitude]
    pub center: [f64; 2],
    pub zoom: f64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Layer {
    /// `street`, `topo` or `aerial`
    pub kind: String,
    /// `mvt`, `png`, `jpeg`, `webp` or `avif`
    pub tile_type: String,
    /// `none`, `gzip` or `zstd`
    pub compression: String,
    pub min_zoom: u8,
    pub max_zoom: u8,
    pub tiles: u64,
    pub data_offset: u64,
    pub data_length: u64,
    pub index_offset: u64,
    pub index_count: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Glyph {
    pub font: String,
    pub range: String,
    pub offset: u64,
    pub length: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Manifest {
    pub format: u32,
    pub name: String,
    #[serde(default)]
    pub created: u64,
    /// [west, south, east, north]
    pub bbox: [f64; 4],
    pub view: View,
    pub layers: Vec<Layer>,
    #[serde(default)]
    pub glyphs: Vec<Glyph>,
    /// The person's own map: pins, route, areas to avoid, drawings, directions. Passed to the screen as it is.
    #[serde(default)]
    pub map: serde_json::Value,
}

/// One entry of a tile list.
type Entry = [u32; 5];

pub struct OpenCmap {
    pub path: PathBuf,
    pub manifest: Manifest,
    /// One tile list per layer (same order as `manifest.layers`), sorted by z, x, y.
    lists: Vec<Vec<Entry>>,
}

/// What a tile or lettering request gives back.
pub struct Piece {
    pub bytes: Vec<u8>,
    /// MIME type
    pub content_type: &'static str,
}

fn plain(msg: &str) -> String {
    msg.to_string()
}

fn read_at(f: &mut File, at: u64, len: usize) -> Result<Vec<u8>, String> {
    f.seek(SeekFrom::Start(at)).map_err(|e| e.to_string())?;
    let mut buf = vec![0u8; len];
    f.read_exact(&mut buf).map_err(|_| plain("This map file is damaged or was not finished copying."))?;
    Ok(buf)
}

fn mime(tile_type: &str) -> &'static str {
    match tile_type {
        "mvt" => "application/x-protobuf",
        "png" => "image/png",
        "jpeg" => "image/jpeg",
        "webp" => "image/webp",
        "avif" => "image/avif",
        _ => "application/octet-stream",
    }
}

/// Read the description at the end of the file, without loading the tile lists.
pub fn read_manifest(path: &Path) -> Result<(Manifest, u64), String> {
    let mut f = File::open(path).map_err(|_| plain("The map file could not be opened."))?;
    let len = f.metadata().map_err(|e| e.to_string())?.len();
    if len < 8 + 12 {
        return Err(plain("This is not a CindrixOps map file."));
    }
    let head = read_at(&mut f, 0, 8)?;
    if head != MAGIC {
        return Err(plain("This is not a CindrixOps map file."));
    }
    let foot = read_at(&mut f, len - 12, 12)?;
    if &foot[4..] != END_MAGIC {
        return Err(plain("This map file is damaged or was not finished copying."));
    }
    let mlen = u32::from_le_bytes([foot[0], foot[1], foot[2], foot[3]]) as u64;
    if mlen == 0 || mlen > MAX_MANIFEST || mlen + 20 > len {
        return Err(plain("This map file is damaged."));
    }
    let start = len - 12 - mlen;
    let raw = read_at(&mut f, start, mlen as usize)?;
    let m: Manifest = serde_json::from_slice(&raw).map_err(|_| plain("This map file's description could not be read."))?;
    if m.format != 1 {
        return Err(plain("This map file was made by a newer CindrixOps. Please update this viewer."));
    }
    let [w, s, e, n] = m.bbox;
    if ![w, s, e, n].iter().all(|v| v.is_finite()) || w >= e || s >= n {
        return Err(plain("This map file's area is not valid."));
    }
    if m.layers.is_empty() {
        return Err(plain("This map file holds no map."));
    }
    // every piece must lie inside the file, before the description
    for l in &m.layers {
        let data_end = l.data_offset.checked_add(l.data_length);
        let index_len = l.index_count.checked_mul(20);
        let index_end = index_len.and_then(|x| l.index_offset.checked_add(x));
        match (data_end, index_end) {
            (Some(d), Some(i)) if d <= start && i <= start && l.data_offset >= 8 && l.index_offset >= 8 && l.index_count <= MAX_TILES => {}
            _ => return Err(plain("This map file is damaged.")),
        }
    }
    for g in &m.glyphs {
        match g.offset.checked_add(g.length) {
            Some(end) if end <= start && g.offset >= 8 && g.length <= MAX_GLYPH => {}
            _ => return Err(plain("This map file is damaged.")),
        }
    }
    Ok((m, len))
}

impl OpenCmap {
    pub fn open(path: &Path) -> Result<OpenCmap, String> {
        let (manifest, _len) = read_manifest(path)?;
        let mut f = File::open(path).map_err(|_| plain("The map file could not be opened."))?;
        let mut lists = Vec::with_capacity(manifest.layers.len());
        for l in &manifest.layers {
            let raw = read_at(&mut f, l.index_offset, (l.index_count * 20) as usize)?;
            let mut list: Vec<Entry> = raw
                .chunks_exact(20)
                .map(|c| {
                    let v = |i: usize| u32::from_le_bytes([c[i * 4], c[i * 4 + 1], c[i * 4 + 2], c[i * 4 + 3]]);
                    [v(0), v(1), v(2), v(3), v(4)]
                })
                .collect();
            // the main program writes them in order; sort anyway so lookups never depend on it
            if !list.windows(2).all(|w| (w[0][0], w[0][1], w[0][2]) <= (w[1][0], w[1][1], w[1][2])) {
                list.sort_by_key(|e| (e[0], e[1], e[2]));
            }
            lists.push(list);
        }
        Ok(OpenCmap { path: path.to_path_buf(), manifest, lists })
    }

    fn layer_index(&self, kind: &str) -> Option<usize> {
        self.manifest.layers.iter().position(|l| l.kind == kind)
    }

    /// The kinds of picture this file carries that this viewer cannot draw yet (zstd tiles need a decoder).
    pub fn unsupported(&self) -> Vec<String> {
        self.manifest.layers.iter().filter(|l| l.compression == "zstd").map(|l| l.kind.clone()).collect()
    }

    /// One tile of one layer, or `None` when the file has no such tile (outside the area or the detail range).
    pub fn tile(&self, kind: &str, z: u32, x: u32, y: u32) -> Result<Option<Piece>, String> {
        let Some(i) = self.layer_index(kind) else { return Ok(None) };
        let l = &self.manifest.layers[i];
        if l.compression == "zstd" {
            return Err(plain("This map uses a kind of packing this viewer cannot open yet."));
        }
        let list = &self.lists[i];
        let Ok(at) = list.binary_search_by_key(&(z, x, y), |e| (e[0], e[1], e[2])) else { return Ok(None) };
        let (off, len) = (list[at][3] as u64, list[at][4] as u64);
        if off + len > l.data_length {
            return Err(plain("This map file is damaged."));
        }
        let mut f = File::open(&self.path).map_err(|_| plain("The map file could not be opened."))?;
        let bytes = read_at(&mut f, l.data_offset + off, len as usize)?;
        // gzip tiles are handed over as they are; the screen unpacks them (src/lib/tiles.ts)
        Ok(Some(Piece { bytes, content_type: mime(&l.tile_type) }))
    }

    /// One lettering file (`range` like `0-255`) for a font, or `None`.
    pub fn glyph(&self, font: &str, range: &str) -> Result<Option<Piece>, String> {
        let Some(g) = self.manifest.glyphs.iter().find(|g| g.font == font && g.range == range) else { return Ok(None) };
        let mut f = File::open(&self.path).map_err(|_| plain("The map file could not be opened."))?;
        let bytes = read_at(&mut f, g.offset, g.length as usize)?;
        Ok(Some(Piece { bytes, content_type: "application/x-protobuf" }))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    fn fixture() -> PathBuf {
        Path::new(env!("CARGO_MANIFEST_DIR")).join("../tests/fixtures/real-core-v1.cmap")
    }

    /// A small file laid out exactly as the spec says, for the cases the real fixture does not cover.
    fn make(manifest_extra: &str, tiles: &[(u32, u32, u32, &[u8])], sorted: bool) -> Vec<u8> {
        let mut out = Vec::new();
        out.extend_from_slice(MAGIC);
        let data_start = out.len() as u64;
        let mut index: Vec<[u32; 5]> = Vec::new();
        for (z, x, y, b) in tiles {
            index.push([*z, *x, *y, (out.len() as u64 - data_start) as u32, b.len() as u32]);
            out.extend_from_slice(b);
        }
        let data_len = out.len() as u64 - data_start;
        if !sorted {
            index.reverse();
        }
        let index_offset = out.len() as u64;
        for e in &index {
            for v in e {
                out.extend_from_slice(&v.to_le_bytes());
            }
        }
        let m = format!(
            "{{\"format\":1,\"name\":\"T\",\"created\":1,\"bbox\":[-80.1,39.4,-80.0,39.5],\"view\":{{\"center\":[39.45,-80.05],\"zoom\":12}},\"layers\":[{{\"kind\":\"aerial\",\"tile_type\":\"jpeg\",\"compression\":\"none\",\"min_zoom\":12,\"max_zoom\":13,\"tiles\":{n},\"data_offset\":{d},\"data_length\":{dl},\"index_offset\":{io},\"index_count\":{n}}}],\"glyphs\":[],\"map\":{{}}{extra}}}",
            n = index.len(), d = data_start, dl = data_len, io = index_offset, extra = manifest_extra
        );
        out.extend_from_slice(m.as_bytes());
        out.extend_from_slice(&(m.len() as u32).to_le_bytes());
        out.extend_from_slice(END_MAGIC);
        out
    }


    /// Change some text inside the manifest and write the footer again, so the file stays well formed.
    fn edit_manifest(b: &[u8], from: &str, to: &str) -> Vec<u8> {
        let n = b.len();
        let m = u32::from_le_bytes([b[n - 12], b[n - 11], b[n - 10], b[n - 9]]) as usize;
        let start = n - 12 - m;
        let text = String::from_utf8(b[start..n - 12].to_vec()).unwrap();
        assert!(text.contains(from), "{} not in manifest", from);
        let text = text.replacen(from, to, 1);
        let mut out = b[..start].to_vec();
        out.extend_from_slice(text.as_bytes());
        out.extend_from_slice(&(text.len() as u32).to_le_bytes());
        out.extend_from_slice(END_MAGIC);
        out
    }

    fn temp(name: &str, bytes: &[u8]) -> PathBuf {
        let d = tempfile::tempdir().unwrap();
        let p = d.path().join(name);
        std::fs::File::create(&p).unwrap().write_all(bytes).unwrap();
        // keep the directory alive for the test by leaking it (the OS cleans the temp folder)
        std::mem::forget(d);
        p
    }

    #[test]
    fn opens_a_file_written_by_the_main_program() {
        let c = OpenCmap::open(&fixture()).unwrap();
        assert_eq!(c.manifest.name, "Trail");
        assert_eq!(c.manifest.layers.len(), 1);
        let l = &c.manifest.layers[0];
        assert_eq!((l.kind.as_str(), l.tile_type.as_str(), l.compression.as_str()), ("street", "mvt", "none"));
        assert_eq!((l.min_zoom, l.max_zoom, l.tiles), (12, 13, 2));
        assert!(c.unsupported().is_empty());
        // the person's own pins come through untouched
        assert_eq!(c.manifest.map["pins"][0]["label"], "Camp");
        assert_eq!(c.manifest.glyphs[0].font, "Noto Sans Regular");
    }

    #[test]
    fn every_listed_tile_is_found_and_other_places_are_not() {
        let c = OpenCmap::open(&fixture()).unwrap();
        let list = &c.lists[0];
        assert_eq!(list.len(), 2);
        for e in list {
            let t = c.tile("street", e[0], e[1], e[2]).unwrap().expect("listed tile");
            assert_eq!(t.bytes.len(), e[4] as usize);
            assert_eq!(t.content_type, "application/x-protobuf");
        }
        assert!(c.tile("street", 14, 1000, 1000).unwrap().is_none());
        assert!(c.tile("topo", list[0][0], list[0][1], list[0][2]).unwrap().is_none());
    }

    #[test]
    fn lettering_is_found_by_font_and_range() {
        let c = OpenCmap::open(&fixture()).unwrap();
        assert_eq!(c.glyph("Noto Sans Regular", "0-255").unwrap().unwrap().bytes, b"glyphs-a");
        assert!(c.glyph("Noto Sans Regular", "256-511").unwrap().is_none());
        assert!(c.glyph("Other", "0-255").unwrap().is_none());
    }

    #[test]
    fn tiles_are_found_even_when_the_list_is_not_in_order() {
        let p = temp("u.cmap", &make("", &[(12, 1, 1, b"aaa"), (12, 1, 2, b"bb"), (13, 5, 5, b"cccc")], false));
        let c = OpenCmap::open(&p).unwrap();
        assert_eq!(c.tile("aerial", 12, 1, 2).unwrap().unwrap().bytes, b"bb");
        assert_eq!(c.tile("aerial", 13, 5, 5).unwrap().unwrap().bytes, b"cccc");
        assert_eq!(c.tile("aerial", 12, 1, 1).unwrap().unwrap().content_type, "image/jpeg");
    }

    #[test]
    fn plain_errors_for_files_that_are_not_maps() {
        let good = std::fs::read(fixture()).unwrap();
        let cases: Vec<(&str, Vec<u8>)> = vec![
            ("empty", Vec::new()),
            ("text", b"hello this is not a map file at all".to_vec()),
            ("no start", good[8..].to_vec()),
            ("cut short", good[..good.len() - 20].to_vec()),
            ("no end mark", { let mut b = good.clone(); let n = b.len(); b[n - 1] = b'X'; b }),
        ];
        for (name, bytes) in cases {
            let p = temp("bad.cmap", &bytes);
            let e = OpenCmap::open(&p).err().unwrap_or_else(|| panic!("{} should fail", name));
            assert!(!e.is_empty() && !e.to_lowercase().contains("panic"), "{}: {}", name, e);
        }
        assert!(OpenCmap::open(Path::new("/no/such/file.cmap")).is_err());
    }

    #[test]
    fn rejects_a_layer_that_points_outside_the_file() {
        let good = make("", &[(12, 1, 1, b"aaa")], true);
        for (from, to) in [("\"data_length\":3", "\"data_length\":99999999"), ("\"index_offset\":11", "\"index_offset\":99999999"), ("\"index_count\":1", "\"index_count\":5000")] {
            let p = temp("far.cmap", &edit_manifest(&good, from, to));
            assert!(OpenCmap::open(&p).is_err(), "{} should be refused", from);
        }
    }

    #[test]
    fn a_newer_format_is_refused_politely() {
        let good = make("", &[(12, 1, 1, b"aaa")], true);
        let p = temp("v2.cmap", &edit_manifest(&good, "\"format\":1", "\"format\":2"));
        let e = OpenCmap::open(&p).err().unwrap();
        assert!(e.contains("newer"), "{}", e);
    }

    #[test]
    fn an_area_that_is_not_a_box_is_refused() {
        let good = make("", &[(12, 1, 1, b"aaa")], true);
        let p = temp("box.cmap", &edit_manifest(&good, "[-80.1,39.4,-80.0,39.5]", "[-80.0,39.4,-80.1,39.5]"));
        assert!(OpenCmap::open(&p).is_err());
    }

    #[test]
    fn a_tile_that_points_past_its_data_is_an_error_not_a_crash() {
        let mut b = make("", &[(12, 1, 1, b"aaa")], true);
        // corrupt the tile length inside the tile list (last u32 of the one entry)
        let n = b.len();
        let m = u32::from_le_bytes([b[n - 12], b[n - 11], b[n - 10], b[n - 9]]) as usize;
        let list_end = n - 12 - m;
        b[list_end - 4..list_end].copy_from_slice(&1_000_000u32.to_le_bytes());
        let p = temp("t.cmap", &b);
        let c = OpenCmap::open(&p).unwrap();
        assert!(c.tile("aerial", 12, 1, 1).is_err());
    }
}
