//! On-disk snapshot of the index so startup does not wait for a full walk.
//!
//! Layout (little endian): magic, version, scope fingerprint, then the
//! `lower` and `originals` arenas (chunk count, then length + bytes per
//! chunk), the packed entries and the `cased` table. A mismatch in any
//! header field means "rebuild".

use std::fs::{self, File};
use std::io::{BufReader, BufWriter, Read, Write};
use std::path::Path;

use anyhow::{Context, Result, bail};

use crate::index::{Arena, Entry, EntryId, FileIndex};

const MAGIC: &[u8; 8] = b"YALTRIDX";
const VERSION: u32 = 3;
/// Sanity limits for a corrupt header (offsets are u32: 1024 x 4 MiB).
const MAX_CHUNKS: usize = 1024;
const MAX_CHUNK_BYTES: usize = 4 << 20;
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
        for arena in [&index.lower, &index.originals] {
            w.write_all(&(arena.chunks.len() as u32).to_le_bytes())?;
            for chunk in &arena.chunks {
                w.write_all(&(chunk.len() as u64).to_le_bytes())?;
                w.write_all(chunk)?;
            }
        }
        w.write_all(&(index.entries.len() as u64).to_le_bytes())?;
        for e in &index.entries {
            w.write_all(&e.name_off.to_le_bytes())?;
            w.write_all(&e.parent.to_le_bytes())?;
            w.write_all(&e.name_len.to_le_bytes())?;
            w.write_all(&[e.flags])?;
        }
        w.write_all(&(index.cased.len() as u64).to_le_bytes())?;
        for (id, off) in &index.cased {
            w.write_all(&id.to_le_bytes())?;
            w.write_all(&off.to_le_bytes())?;
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
    let lower = read_arena(&mut r)?;
    let originals = read_arena(&mut r)?;
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
        let valid = e.name_off >= prev_off
            && valid_name(&lower, e.name_off, e.name_len);
        if !valid {
            bail!("corrupt index: bad entry");
        }
        prev_off = e.name_off;
        entries.push(e);
    }
    drop(raw);

    let cased_count = read_u64(&mut r)? as usize;
    if cased_count != entries.iter().filter(|e| e.is_cased()).count() {
        bail!("corrupt index: cased table mismatch");
    }
    let mut raw = vec![0u8; cased_count.checked_mul(8).context("corrupt index")?];
    r.read_exact(&mut raw)?;
    let mut cased: Vec<(EntryId, u32)> = Vec::with_capacity(cased_count);
    for chunk in raw.chunks_exact(8) {
        let id = u32::from_le_bytes(chunk[0..4].try_into()?);
        let off = u32::from_le_bytes(chunk[4..8].try_into()?);
        let valid = cased.last().is_none_or(|&(prev, _)| id > prev)
            && entries.get(id as usize).is_some_and(|e| e.is_cased() && valid_name(&originals, off, e.name_len));
        if !valid {
            bail!("corrupt index: bad cased entry");
        }
        cased.push((id, off));
    }
    Ok(FileIndex::from_parts(lower, originals, cased, entries))
}

/// `len` bytes of UTF-8 followed by the `\0` terminator at `off`.
fn valid_name(arena: &Arena, off: u32, len: u16) -> bool {
    arena.get(off, len as usize + 1).is_some()
        && arena.get(off, len as usize).is_some_and(|b| std::str::from_utf8(b).is_ok())
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

fn read_arena(r: &mut impl Read) -> Result<Arena> {
    let count = read_u32(r)? as usize;
    if count > MAX_CHUNKS {
        bail!("corrupt index: too many chunks");
    }
    let mut chunks = Vec::with_capacity(count);
    for _ in 0..count {
        let len = read_u64(r)? as usize;
        if len > MAX_CHUNK_BYTES {
            bail!("corrupt index: chunk too large");
        }
        let mut chunk = vec![0u8; len];
        r.read_exact(&mut chunk)?;
        chunks.push(chunk);
    }
    Ok(Arena { chunks })
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
        assert_eq!(back.lower, idx.lower);
        assert_eq!(back.originals, idx.originals);
        assert_eq!(back.cased, idx.cased);
        assert_eq!(back.path_of(2).unwrap(), Path::new("/r/Klasör/Dosya.txt"));
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
