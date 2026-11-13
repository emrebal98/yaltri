//! On-disk snapshot of the index so startup does not wait for a full walk.
//!
//! Layout (little endian): magic, version, scope fingerprint, then the
//! `names` and `lower` arenas and the packed entries. A mismatch in any
//! header field means "rebuild".

use std::fs::{self, File};
use std::io::{BufReader, BufWriter, Read, Write};
use std::path::Path;

use anyhow::{Context, Result, bail};

use crate::index::{Entry, FileIndex};

const MAGIC: &[u8; 8] = b"YALTRIDX";
const VERSION: u32 = 1;
const ENTRY_BYTES: usize = 11;

pub fn save(index: &FileIndex, fingerprint: u64, path: &Path) -> Result<()> {
    if let Some(dir) = path.parent() {
        fs::create_dir_all(dir)?;
    }
    let tmp = path.with_extension("idx.tmp");
    {
        let mut w = BufWriter::new(File::create(&tmp).with_context(|| format!("creating {}", tmp.display()))?);
        w.write_all(MAGIC)?;
        w.write_all(&VERSION.to_le_bytes())?;
        w.write_all(&fingerprint.to_le_bytes())?;
        for arena in [&index.names, &index.lower] {
            w.write_all(&(arena.len() as u64).to_le_bytes())?;
            w.write_all(arena)?;
        }
        w.write_all(&(index.entries.len() as u64).to_le_bytes())?;
        for e in &index.entries {
            w.write_all(&e.name_off.to_le_bytes())?;
            w.write_all(&e.parent.to_le_bytes())?;
            w.write_all(&e.name_len.to_le_bytes())?;
            w.write_all(&[e.flags])?;
        }
        w.flush()?;
    }
    fs::rename(&tmp, path)?;
    Ok(())
}

/// Loads a snapshot written for the same scope `fingerprint`.
pub fn load(path: &Path, fingerprint: u64) -> Result<FileIndex> {
    let mut r = BufReader::new(File::open(path)?);
    let mut magic = [0u8; 8];
    r.read_exact(&mut magic)?;
    if &magic != MAGIC {
        bail!("not a yaltri index");
    }
    if read_u32(&mut r)? != VERSION {
        bail!("index version changed");
    }
    if read_u64(&mut r)? != fingerprint {
        bail!("index scope changed");
    }
    let names = read_arena(&mut r)?;
    let lower = read_arena(&mut r)?;
    if names.len() != lower.len() {
        bail!("corrupt index: arena mismatch");
    }
    let count = read_u64(&mut r)? as usize;
    let mut raw = vec![0u8; count.checked_mul(ENTRY_BYTES).context("corrupt index")?];
    r.read_exact(&mut raw)?;
    let mut entries = Vec::with_capacity(count);
    let mut prev_off = 0u32;
    for chunk in raw.chunks_exact(ENTRY_BYTES) {
        let e = Entry {
            name_off: u32::from_le_bytes(chunk[0..4].try_into()?),
            parent: u32::from_le_bytes(chunk[4..8].try_into()?),
            name_len: u16::from_le_bytes(chunk[8..10].try_into()?),
            flags: chunk[10],
        };
        let end = e.name_off as usize + e.name_len as usize;
        if e.name_off < prev_off || end >= names.len() || std::str::from_utf8(&names[e.name_off as usize..end]).is_err() {
            bail!("corrupt index: bad entry");
        }
        prev_off = e.name_off;
        entries.push(e);
    }
    Ok(FileIndex::from_parts(names, lower, entries))
}

fn read_u32(r: &mut impl Read) -> Result<u32> {
    let mut b = [0u8; 4];
    r.read_exact(&mut b)?;
    Ok(u32::from_le_bytes(b))
}

fn read_u64(r: &mut impl Read) -> Result<u64> {
    let mut b = [0u8; 8];
    r.read_exact(&mut b)?;
    Ok(u64::from_le_bytes(b))
}

fn read_arena(r: &mut impl Read) -> Result<Vec<u8>> {
    let len = read_u64(r)? as usize;
    // Guard against absurd sizes from a corrupt header (4 GiB arena limit:
    // offsets are u32).
    if len > u32::MAX as usize {
        bail!("corrupt index: arena too large");
    }
    let mut v = vec![0u8; len];
    r.read_exact(&mut v)?;
    Ok(v)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample() -> FileIndex {
        let mut idx = FileIndex::new();
        let root = idx.add_root(Path::new("/r"));
        let d = idx.push(root, "Klasör", true);
        idx.push(d, "Dosya.txt", false);
        let gone = idx.push(d, "gone.txt", false);
        idx.mark_deleted(gone);
        idx
    }

    fn tmp(name: &str) -> std::path::PathBuf {
        std::env::temp_dir().join(format!("yaltri-persist-{name}-{}.idx", std::process::id()))
    }

    #[test]
    fn round_trip() {
        let idx = sample();
        let path = tmp("rt");
        save(&idx, 42, &path).unwrap();
        let back = load(&path, 42).unwrap();
        assert_eq!(back.entries, idx.entries);
        assert_eq!(back.names, idx.names);
        assert_eq!(back.len(), idx.len());
        assert_eq!(back.search("dosya", 5, &|_| 0).len(), 1);
        fs::remove_file(path).unwrap();
    }

    #[test]
    fn rejects_other_scope_and_garbage() {
        let path = tmp("bad");
        save(&sample(), 1, &path).unwrap();
        assert!(load(&path, 2).is_err());
        fs::write(&path, b"YALTRIDX\x01\x00\x00\x00garbage").unwrap();
        assert!(load(&path, 1).is_err());
        fs::remove_file(path).unwrap();
    }
}
