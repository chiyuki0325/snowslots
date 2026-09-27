use std::io::{self, Cursor, Read};
use std::path::{Component, Path, PathBuf};

use crate::{atomic_write, Cart, Platform};

const MAGIC: &[u8; 8] = b"SLLIB\0\0\x01";
const MAX_CARTS: u32 = 100_000;
const MAX_STRING_BYTES: u32 = 16_384;

fn path(root: &Path) -> PathBuf {
    root.join("System").join("library-cache.bin")
}

pub fn read_library_cache(root: &Path) -> io::Result<Option<Vec<Cart>>> {
    let bytes = match std::fs::read(path(root)) {
        Ok(bytes) => bytes,
        Err(e) if e.kind() == io::ErrorKind::NotFound => return Ok(None),
        Err(e) => return Err(e),
    };
    decode(root, &bytes).map(Some)
}

pub fn write_library_cache(root: &Path, carts: &[Cart]) -> io::Result<()> {
    std::fs::create_dir_all(root.join("System"))?;
    atomic_write(&path(root), &encode(carts)?)
}

fn encode(carts: &[Cart]) -> io::Result<Vec<u8>> {
    let mut out = Vec::new();
    out.extend_from_slice(MAGIC);
    out.extend_from_slice(&(carts.len() as u32).to_le_bytes());
    for cart in carts {
        let file = cart
            .rom
            .file_name()
            .and_then(|name| name.to_str())
            .ok_or_else(|| invalid("rom filename is not utf-8"))?;
        out.push(cart.platform.code());
        push_string(&mut out, &cart.stem)?;
        push_string(&mut out, file)?;
        out.push(u8::from(cart.label.is_some()));
        push_string(&mut out, &cart.title)?;
        push_string(&mut out, &cart.code)?;
    }
    Ok(out)
}

fn decode(root: &Path, bytes: &[u8]) -> io::Result<Vec<Cart>> {
    let mut r = Cursor::new(bytes);
    let mut magic = [0; MAGIC.len()];
    r.read_exact(&mut magic)?;
    if &magic != MAGIC {
        return Err(invalid("unknown library cache format"));
    }
    let count = read_u32(&mut r)?;
    if count > MAX_CARTS {
        return Err(invalid("too many cached carts"));
    }
    let mut carts = Vec::with_capacity(count as usize);
    for _ in 0..count {
        let platform = Platform::from_code(read_u8(&mut r)?)
            .ok_or_else(|| invalid("unknown cached platform"))?;
        let stem = read_string(&mut r)?;
        let file = read_string(&mut r)?;
        if !single_component(&file) {
            return Err(invalid("cached rom filename is not one path component"));
        }
        let has_label = match read_u8(&mut r)? {
            0 => false,
            1 => true,
            _ => return Err(invalid("invalid cached label flag")),
        };
        let title = read_string(&mut r)?;
        let code = read_string(&mut r)?;
        let rom = root.join("Games").join(platform.dir_name()).join(file);
        let label = has_label.then(|| {
            root.join("Labels")
                .join(platform.dir_name())
                .join(format!("{stem}.png"))
        });
        carts.push(Cart {
            stem,
            platform,
            rom,
            label,
            title,
            code,
            last_launched: None,
        });
    }
    if r.position() != bytes.len() as u64 {
        return Err(invalid("trailing library cache bytes"));
    }
    Ok(carts)
}

fn push_string(out: &mut Vec<u8>, value: &str) -> io::Result<()> {
    let len = u32::try_from(value.len()).map_err(|_| invalid("cached string is too long"))?;
    if len > MAX_STRING_BYTES {
        return Err(invalid("cached string is too long"));
    }
    out.extend_from_slice(&len.to_le_bytes());
    out.extend_from_slice(value.as_bytes());
    Ok(())
}

fn read_string(r: &mut Cursor<&[u8]>) -> io::Result<String> {
    let len = read_u32(r)?;
    if len > MAX_STRING_BYTES {
        return Err(invalid("cached string is too long"));
    }
    let mut bytes = vec![0; len as usize];
    r.read_exact(&mut bytes)?;
    String::from_utf8(bytes).map_err(|_| invalid("cached string is not utf-8"))
}

fn single_component(value: &str) -> bool {
    let mut components = Path::new(value).components();
    matches!(components.next(), Some(Component::Normal(_))) && components.next().is_none()
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

fn invalid(message: &'static str) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidData, message)
}
