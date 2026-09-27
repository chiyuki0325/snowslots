use std::ffi::OsStr;
use std::fmt;
use std::path::{Path, PathBuf};

use pinyin::ToPinyin;

use crate::gba::{header_code, header_title};
use crate::library_cache::{read_library_cache, write_library_cache};
use crate::{read_play_history, Platform};

#[derive(Clone, PartialEq, Eq, Debug)]
pub struct Cart {
    /// Filename stem, which is the key for labels, saves and states. Not a content hash.
    pub stem: String,
    pub platform: Platform,
    pub rom: PathBuf,
    pub label: Option<PathBuf>,
    pub title: String,
    /// The four character header game code, empty when the rom has none.
    pub code: String,
    /// UTC seconds since the Unix epoch. `None` means this cart has not launched successfully.
    pub last_launched: Option<i64>,
}

#[derive(Copy, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Debug)]
pub enum NameGroup {
    Digit(u8),
    Letter(u8),
    Other,
}

#[derive(Clone, PartialEq, Eq, PartialOrd, Ord, Debug)]
pub struct NameSortKey {
    group: NameGroup,
    folded: String,
    original: String,
}

#[derive(Debug)]
pub enum StoreError {
    Io(std::io::Error),
}

impl fmt::Display for StoreError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            StoreError::Io(e) => write!(f, "io: {e}"),
        }
    }
}

impl std::error::Error for StoreError {}

impl From<std::io::Error> for StoreError {
    fn from(e: std::io::Error) -> Self {
        StoreError::Io(e)
    }
}

pub struct ScanResult {
    pub carts: Vec<Cart>,
    pub from_cache: bool,
}

/// Loads the last complete library immediately. A cache miss performs the full scan so first boot
/// still opens on a truthful shelf rather than briefly claiming the card is empty.
pub fn scan_fast(root: &Path) -> Result<ScanResult, StoreError> {
    let history = read_play_history(root).unwrap_or_default();
    if let Ok(Some(mut carts)) = read_library_cache(root) {
        merge_history(&mut carts, &history);
        carts.sort_by_key(|c| name_sort_key(&c.stem));
        return Ok(ScanResult {
            carts,
            from_cache: true,
        });
    }
    let carts = scan_fresh(root, &history)?;
    let _ = write_library_cache(root, &carts);
    Ok(ScanResult {
        carts,
        from_cache: false,
    })
}

/// Compatibility entry point for callers that do not need to know whether a refresh is due.
pub fn scan(root: &Path) -> Result<Vec<Cart>, StoreError> {
    Ok(scan_fast(root)?.carts)
}

/// Reads the card itself, then replaces the disposable library cache.
pub fn refresh(root: &Path) -> Result<Vec<Cart>, StoreError> {
    let history = read_play_history(root).unwrap_or_default();
    let carts = scan_fresh(root, &history)?;
    if let Err(e) = write_library_cache(root, &carts) {
        eprintln!("slot: library cache: {e}");
    }
    Ok(carts)
}

fn scan_fresh(root: &Path, history: &crate::PlayHistory) -> Result<Vec<Cart>, StoreError> {
    let mut carts = Vec::new();
    for platform in Platform::ALL {
        let dir = root.join("Games").join(platform.dir_name());
        let entries = match std::fs::read_dir(&dir) {
            Ok(d) => d,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => continue,
            Err(e) => {
                eprintln!("slot: scan: {}: {e}", dir.display());
                continue;
            }
        };
        for entry in entries {
            let Ok(entry) = entry else {
                continue;
            };
            let rom = entry.path();
            if is_hidden(&rom) || !rom.is_file() || !is_rom(platform, &rom) {
                continue;
            }
            let Some(name) = rom.file_name().and_then(decode_filename) else {
                continue;
            };
            let Some(stem) = Path::new(&name)
                .file_stem()
                .and_then(|s| s.to_str())
                .map(str::to_string)
            else {
                continue;
            };
            let label = root
                .join("Labels")
                .join(platform.dir_name())
                .join(format!("{stem}.png"));
            carts.push(Cart {
                stem: stem.clone(),
                platform,
                title: header_title(&rom).unwrap_or_default(),
                code: header_code(&rom).unwrap_or_default(),
                label: label.is_file().then_some(label),
                rom,
                last_launched: history.get(&(platform, stem)).copied(),
            });
        }
    }
    carts.sort_by_key(|c| name_sort_key(&c.stem));
    Ok(carts)
}

fn merge_history(carts: &mut [Cart], history: &crate::PlayHistory) {
    for cart in carts {
        cart.last_launched = history.get(&(cart.platform, cart.stem.clone())).copied();
    }
}

fn is_rom(platform: Platform, path: &Path) -> bool {
    let expected = match platform {
        Platform::Gba => "gba",
    };
    path.file_name()
        .and_then(decode_filename)
        .and_then(|name| {
            Path::new(&name)
                .extension()
                .and_then(|e| e.to_str())
                .map(str::to_string)
        })
        .is_some_and(|ext| ext.eq_ignore_ascii_case(expected))
}

fn decode_filename(name: &OsStr) -> Option<String> {
    let decoded = decode_filename_bytes(filename_bytes(name)?)?;
    Some(recover_utf8_mojibake(&decoded).unwrap_or(decoded))
}

#[cfg(unix)]
fn filename_bytes(name: &OsStr) -> Option<&[u8]> {
    use std::os::unix::ffi::OsStrExt;

    Some(name.as_bytes())
}

#[cfg(not(unix))]
fn filename_bytes(name: &OsStr) -> Option<&[u8]> {
    name.to_str().map(str::as_bytes)
}

fn decode_filename_bytes(bytes: &[u8]) -> Option<String> {
    if let Ok(name) = std::str::from_utf8(bytes) {
        return Some(name.to_string());
    }
    encoding_rs::GB18030
        .decode_without_bom_handling_and_without_replacement(bytes)
        .map(|name| name.into_owned())
}

fn recover_utf8_mojibake(name: &str) -> Option<String> {
    if name.chars().any(is_cjk) {
        return None;
    }
    let bytes: Option<Vec<u8>> = name.chars().map(|c| u8::try_from(c as u32).ok()).collect();
    let recovered = String::from_utf8(bytes?).ok()?;
    recovered.chars().any(is_cjk).then_some(recovered)
}

fn is_cjk(c: char) -> bool {
    matches!(
        c,
        '\u{3400}'..='\u{4dbf}'
            | '\u{4e00}'..='\u{9fff}'
            | '\u{f900}'..='\u{faff}'
            | '\u{20000}'..='\u{2fa1f}'
    )
}

pub fn name_sort_key(stem: &str) -> NameSortKey {
    let trimmed = stem.trim_start_matches(char::is_whitespace);
    let mut folded = String::new();
    for c in trimmed.chars() {
        if let Some(pinyin) = c.to_pinyin() {
            folded.push_str(&pinyin.plain().to_uppercase());
        } else {
            folded.extend(c.to_uppercase());
        }
    }
    NameSortKey {
        group: name_group(stem),
        folded,
        original: stem.to_string(),
    }
}

/// Kept as the public spelling used by older callers and tests.
pub fn sort_key(stem: &str) -> NameSortKey {
    name_sort_key(stem)
}

pub fn name_group(stem: &str) -> NameGroup {
    let Some(c) = stem.chars().find(|c| !c.is_whitespace()) else {
        return NameGroup::Other;
    };
    if let Some(digit) = c.to_digit(10).filter(|_| c.is_ascii_digit()) {
        return NameGroup::Digit(digit as u8);
    }
    if let Some(pinyin) = c.to_pinyin() {
        return pinyin
            .first_letter()
            .bytes()
            .next()
            .map(|c| NameGroup::Letter(c.to_ascii_uppercase() - b'A'))
            .unwrap_or(NameGroup::Other);
    }
    let c = c.to_ascii_uppercase();
    if c.is_ascii_alphabetic() {
        NameGroup::Letter(c as u8 - b'A')
    } else {
        NameGroup::Other
    }
}

/// Compatibility spelling for the shelf's former letter-only grouping.
pub fn initial(stem: &str) -> char {
    match name_group(stem) {
        NameGroup::Digit(d) => char::from(b'0' + d),
        NameGroup::Letter(l) => char::from(b'A' + l),
        NameGroup::Other => '#',
    }
}

/// A leading dot is card metadata rather than content, and every folder on the card is read
/// through this. macOS writes `._<name>` beside each file it copies onto a FAT volume, which
/// carries the extension of the file it shadows, so the extension alone cannot tell them
/// apart. It also sorts first, which is why the sidecar rather than the file is what a picker
/// walking the folder in order tends to land on.
pub fn is_hidden(p: &Path) -> bool {
    p.file_name()
        .and_then(|n| n.to_str())
        .is_some_and(|n| n.starts_with('.'))
}
