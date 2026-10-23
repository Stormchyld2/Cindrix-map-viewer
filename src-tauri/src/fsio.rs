use std::{fs, io::{self, Write}, path::Path, time::{SystemTime, UNIX_EPOCH}};
pub fn write_atomic(path: &Path, data: &[u8]) -> io::Result<()> {
    if let Some(dir) = path.parent() { fs::create_dir_all(dir)?; }
    let tmp = path.with_extension("tmp");
    { let mut f = fs::File::create(&tmp)?; f.write_all(data)?; f.sync_all()?; }
    fs::rename(tmp, path)
}
pub fn backup_then_write(path: &Path, backups_dir: &Path, data: &[u8]) -> io::Result<()> {
    if path.exists() {
        fs::create_dir_all(backups_dir)?;
        let secs = SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or(0);
        let name = path.file_name().unwrap_or_default().to_string_lossy();
        fs::copy(path, backups_dir.join(format!("{secs}-{name}")))?;
    }
    write_atomic(path, data)
}
