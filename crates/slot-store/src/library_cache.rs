use std::ffi::OsString;
use std::io::{self, Cursor, Read};
use std::path::{Component, Path, PathBuf};

use crate::scan::{decode_filename, prepare_library};
use crate::{atomic_write, scan_report, Cart, Platform};

const MAGIC: &[u8; 8] = b"SLLIB\0\0\x03";
const MAX_CARTS: usize = 100_000;
const MAX_NAME: usize = 16_384;
const MAX_BYTES: usize = 32 * 1024 * 1024;

pub struct LibraryBoot {
    pub carts: Vec<Cart>,
    pub from_cache: bool,
    pub complete: bool,
}

pub fn library_cache_path(root: &Path) -> PathBuf {
    root.join("Cache/library.bin")
}

pub fn remove_library_cache(root: &Path) -> io::Result<()> {
    match std::fs::remove_file(library_cache_path(root)) {
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(()),
        result => result,
    }
}

pub fn load_library(root: &Path) -> LibraryBoot {
    load_library_with(root, || {})
}

pub fn load_library_with(root: &Path, before_scan: impl FnOnce()) -> LibraryBoot {
    let _ = std::fs::remove_file(root.join("System/library-cache.bin"));
    match read_library_cache(root) {
        Ok(Some(carts)) => {
            return LibraryBoot {
                carts,
                from_cache: true,
                complete: true,
            };
        }
        Ok(None) => {}
        Err(error) => {
            eprintln!("slot: library cache: {error}");
            let _ = remove_library_cache(root);
        }
    }
    before_scan();
    let scanned = scan_report(root);
    LibraryBoot {
        carts: scanned.carts,
        from_cache: false,
        complete: scanned.complete,
    }
}

pub fn write_library_cache(root: &Path, carts: &[Cart]) -> io::Result<()> {
    if carts.len() > MAX_CARTS {
        return Err(invalid("too many games"));
    }
    let mut bytes = MAGIC.to_vec();
    bytes.extend_from_slice(&(carts.len() as u32).to_le_bytes());
    for cart in carts {
        push_name(&mut bytes, cart.platform.dir_name().as_bytes())?;
        push_path(&mut bytes, &cart.rom)?;
        match &cart.label {
            Some(label) => {
                bytes.push(1);
                push_path(&mut bytes, label)?;
            }
            None => bytes.push(0),
        }
        if bytes.len() > MAX_BYTES {
            return Err(invalid("library cache is too large"));
        }
    }
    let path = library_cache_path(root);
    if std::fs::read(&path).ok().as_deref() == Some(bytes.as_slice()) {
        return Ok(());
    }
    std::fs::create_dir_all(root.join("Cache"))?;
    atomic_write(&path, &bytes)
}

pub fn read_library_cache(root: &Path) -> io::Result<Option<Vec<Cart>>> {
    let file = match std::fs::File::open(library_cache_path(root)) {
        Ok(file) => file,
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(None),
        Err(error) => return Err(error),
    };
    let mut bytes = Vec::new();
    file.take((MAX_BYTES + 1) as u64).read_to_end(&mut bytes)?;
    if bytes.len() > MAX_BYTES {
        return Err(invalid("library cache is too large"));
    }
    let mut input = Cursor::new(bytes.as_slice());
    let mut magic = [0; 8];
    input.read_exact(&mut magic)?;
    if &magic != MAGIC {
        return Err(invalid("unknown library cache version"));
    }
    let count = read_u32(&mut input)? as usize;
    if count > MAX_CARTS || count > bytes.len() / 10 {
        return Err(invalid("invalid game count"));
    }
    let mut carts = Vec::with_capacity(count);
    for _ in 0..count {
        let dir = read_name(&mut input)?;
        let platform = Platform::ALL
            .into_iter()
            .find(|platform| platform.dir_name().as_bytes() == dir)
            .ok_or_else(|| invalid("unknown platform"))?;
        let rom = root
            .join("Games")
            .join(platform.dir_name())
            .join(read_filename(&mut input)?);
        if !platform.accepts(&rom) || crate::is_hidden(&rom) {
            return Err(invalid("invalid game filename"));
        }
        let stem = rom
            .file_stem()
            .and_then(decode_filename)
            .ok_or_else(|| invalid("invalid game name"))?;
        let mut flag = [0];
        input.read_exact(&mut flag)?;
        let label = match flag[0] {
            0 => None,
            1 => Some(
                root.join("Labels")
                    .join(platform.dir_name())
                    .join(read_filename(&mut input)?),
            ),
            _ => return Err(invalid("invalid label flag")),
        };
        carts.push(Cart {
            platform,
            stem,
            rom,
            label,
            title: String::new(),
            code: String::new(),
            shell: None,
            tags: Vec::new(),
        });
    }
    if input.position() != bytes.len() as u64 {
        return Err(invalid("trailing library cache data"));
    }
    prepare_library(root, &mut carts)?;
    Ok(Some(carts))
}

fn push_path(bytes: &mut Vec<u8>, path: &Path) -> io::Result<()> {
    let name = path
        .file_name()
        .ok_or_else(|| invalid("missing filename"))?;
    push_name(bytes, name.as_encoded_bytes())
}

fn push_name(bytes: &mut Vec<u8>, name: &[u8]) -> io::Result<()> {
    if name.len() > MAX_NAME {
        return Err(invalid("filename is too long"));
    }
    bytes.extend_from_slice(&(name.len() as u32).to_le_bytes());
    bytes.extend_from_slice(name);
    Ok(())
}

fn read_u32(input: &mut Cursor<&[u8]>) -> io::Result<u32> {
    let mut bytes = [0; 4];
    input.read_exact(&mut bytes)?;
    Ok(u32::from_le_bytes(bytes))
}

fn read_name(input: &mut Cursor<&[u8]>) -> io::Result<Vec<u8>> {
    let len = read_u32(input)? as usize;
    if len > MAX_NAME {
        return Err(invalid("filename is too long"));
    }
    let mut bytes = vec![0; len];
    input.read_exact(&mut bytes)?;
    Ok(bytes)
}

fn read_filename(input: &mut Cursor<&[u8]>) -> io::Result<OsString> {
    let bytes = read_name(input)?;
    #[cfg(unix)]
    let name = {
        use std::os::unix::ffi::OsStringExt;
        OsString::from_vec(bytes)
    };
    #[cfg(not(unix))]
    let name =
        OsString::from(String::from_utf8(bytes).map_err(|_| invalid("invalid filename encoding"))?);
    let mut parts = Path::new(&name).components();
    if !matches!(parts.next(), Some(Component::Normal(_)))
        || parts.next().is_some()
        || name.as_encoded_bytes().contains(&0)
    {
        return Err(invalid("filename is not a single path component"));
    }
    Ok(name)
}

fn invalid(message: &'static str) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidData, message)
}
