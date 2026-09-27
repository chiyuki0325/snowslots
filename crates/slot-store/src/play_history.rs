use std::collections::BTreeMap;
use std::io::{self, Cursor, Read};
use std::path::{Path, PathBuf};

use crate::{atomic_write, Platform};

const MAGIC: &[u8; 8] = b"SLPLAY\0\x01";
const MAX_RECORDS: u32 = 100_000;
const MAX_STEM_BYTES: u32 = 4_096;

pub type PlayHistory = BTreeMap<(Platform, String), i64>;

fn path(root: &Path) -> PathBuf {
    root.join("System").join("play-history.bin")
}

pub fn read_play_history(root: &Path) -> io::Result<PlayHistory> {
    let bytes = match std::fs::read(path(root)) {
        Ok(bytes) => bytes,
        Err(e) if e.kind() == io::ErrorKind::NotFound => return Ok(PlayHistory::new()),
        Err(e) => return Err(e),
    };
    decode(&bytes)
}

pub fn write_play_history(root: &Path, history: &PlayHistory) -> io::Result<()> {
    std::fs::create_dir_all(root.join("System"))?;
    atomic_write(&path(root), &encode(history))
}

pub fn write_last_played(
    root: &Path,
    platform: Platform,
    stem: &str,
    utc_secs: i64,
) -> io::Result<()> {
    if utc_secs < 0 {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "last played time is before the Unix epoch",
        ));
    }
    let mut history = read_play_history(root).unwrap_or_default();
    history.insert((platform, stem.to_string()), utc_secs);
    write_play_history(root, &history)
}

fn encode(history: &PlayHistory) -> Vec<u8> {
    let mut out = Vec::new();
    out.extend_from_slice(MAGIC);
    out.extend_from_slice(&(history.len() as u32).to_le_bytes());
    for ((platform, stem), timestamp) in history {
        out.push(platform.code());
        out.extend_from_slice(&(stem.len() as u32).to_le_bytes());
        out.extend_from_slice(stem.as_bytes());
        out.extend_from_slice(&timestamp.to_le_bytes());
    }
    out
}

fn decode(bytes: &[u8]) -> io::Result<PlayHistory> {
    let mut r = Cursor::new(bytes);
    let mut magic = [0; MAGIC.len()];
    r.read_exact(&mut magic)?;
    if &magic != MAGIC {
        return Err(invalid("unknown play history format"));
    }
    let count = read_u32(&mut r)?;
    if count > MAX_RECORDS {
        return Err(invalid("too many play history records"));
    }
    let mut history = PlayHistory::new();
    for _ in 0..count {
        let platform = Platform::from_code(read_u8(&mut r)?)
            .ok_or_else(|| invalid("unknown platform in play history"))?;
        let stem = read_string(&mut r)?;
        let timestamp = read_i64(&mut r)?;
        if timestamp < 0 {
            return Err(invalid("negative play history timestamp"));
        }
        history.insert((platform, stem), timestamp);
    }
    if r.position() != bytes.len() as u64 {
        return Err(invalid("trailing play history bytes"));
    }
    Ok(history)
}

fn read_string(r: &mut Cursor<&[u8]>) -> io::Result<String> {
    let len = read_u32(r)?;
    if len > MAX_STEM_BYTES {
        return Err(invalid("play history stem is too long"));
    }
    let mut bytes = vec![0; len as usize];
    r.read_exact(&mut bytes)?;
    String::from_utf8(bytes).map_err(|_| invalid("play history stem is not utf-8"))
}

fn read_u8(r: &mut Cursor<&[u8]>) -> io::Result<u8> {
    let mut bytes = [0; 1];
    r.read_exact(&mut bytes)?;
    Ok(bytes[0])
}

fn read_u32(r: &mut Cursor<&[u8]>) -> io::Result<u32> {
    let mut bytes = [0; 4];
    r.read_exact(&mut bytes)?;
    Ok(u32::from_le_bytes(bytes))
}

fn read_i64(r: &mut Cursor<&[u8]>) -> io::Result<i64> {
    let mut bytes = [0; 8];
    r.read_exact(&mut bytes)?;
    Ok(i64::from_le_bytes(bytes))
}

fn invalid(message: &'static str) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidData, message)
}
