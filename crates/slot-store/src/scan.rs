use std::collections::HashMap;
use std::ffi::OsStr;
use std::fmt;
use std::path::{Path, PathBuf};

use crate::gba::header_title_code;
use crate::platform::Platform;

#[derive(Clone, PartialEq, Eq, Debug)]
pub struct Cart {
    pub platform: Platform,
    pub stem: String,
    pub rom: PathBuf,
    pub label: Option<PathBuf>,
    pub title: String,
    pub code: String,
    pub shell: Option<crate::ShellChoice>,
    pub tags: Vec<String>,
}

impl Cart {
    pub fn read_header(&mut self) {
        match self.platform.hardware() {
            Platform::Gba => {
                if let Some((title, code)) = header_title_code(&self.rom) {
                    self.title = title;
                    self.code = code;
                }
            }
            _ => {
                if let Some(title) = crate::gb::title(&self.rom) {
                    self.title = title;
                }
            }
        }
    }
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

pub fn scan(root: &Path) -> Result<Vec<Cart>, StoreError> {
    Ok(scan_report(root).carts)
}

pub fn scan_platform(root: &Path, platform: Platform) -> Result<Vec<Cart>, StoreError> {
    Ok(scan_of(root, &[platform]).carts)
}

pub struct LibraryScan {
    pub carts: Vec<Cart>,
    pub complete: bool,
}

pub fn scan_report(root: &Path) -> LibraryScan {
    scan_of(root, &Platform::ALL)
}

fn scan_of(root: &Path, platforms: &[Platform]) -> LibraryScan {
    let mut complete = true;
    let mut carts = Vec::new();
    for platform in platforms.iter().copied() {
        let dir = root.join("Games").join(platform.dir_name());
        let entries = match std::fs::read_dir(&dir) {
            Ok(d) => d,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => continue,
            Err(e) => {
                eprintln!("slot: scan: {}: {e}", dir.display());
                complete = false;
                continue;
            }
        };
        let labels_dir = root.join("Labels").join(platform.dir_name());
        let mut labels = HashMap::new();
        match std::fs::read_dir(&labels_dir) {
            Ok(entries) => {
                for entry in entries {
                    let Ok(entry) = entry else {
                        complete = false;
                        continue;
                    };
                    if !is_hidden(&entry.path()) && is_file(&entry, &mut complete) {
                        if let Some(name) = decode_filename(&entry.file_name()) {
                            labels.insert(name.to_lowercase(), entry.path());
                        }
                    }
                }
            }
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
            Err(_) => complete = false,
        }
        for entry in entries {
            let Ok(entry) = entry else {
                complete = false;
                continue;
            };
            let rom = entry.path();
            if is_hidden(&rom) || !platform.accepts(&rom) || !is_file(&entry, &mut complete) {
                continue;
            }
            let Some(stem) = rom.file_stem().and_then(decode_filename) else {
                continue;
            };
            let label = labels.get(&format!("{stem}.png").to_lowercase()).cloned();
            carts.push(Cart {
                platform,
                stem: stem.to_string(),
                title: String::new(),
                code: String::new(),
                shell: None,
                tags: Vec::new(),
                label,
                rom,
            });
        }
    }
    if prepare_library(root, &mut carts).is_err() {
        complete = false;
    }
    LibraryScan { carts, complete }
}

pub(crate) fn prepare_library(root: &Path, carts: &mut [Cart]) -> std::io::Result<()> {
    let read = |file: &str| match std::fs::read_to_string(root.join(file)) {
        Ok(text) => Ok(text),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(String::new()),
        Err(error) => Err(error),
    };
    let system = read(crate::CART_SHELL_FILE);
    let labels = read(crate::LABELS_SHELL_FILE);
    let shells = crate::cart_shell::layered(
        system.as_deref().unwrap_or_default(),
        labels.as_deref().unwrap_or_default(),
    );
    for cart in carts.iter_mut() {
        cart.shell = shells.get(&crate::cart_shell::key(&cart.stem)).copied();
        cart.tags.clear();
    }
    carts.sort_by(|a, b| {
        (a.platform as u8, sort_key(&a.stem)).cmp(&(b.platform as u8, sort_key(&b.stem)))
    });
    tell_twins_apart(carts);
    system?;
    labels?;
    Ok(())
}

fn tell_twins_apart(carts: &mut [Cart]) {
    let mut twins: HashMap<(Platform, String), Vec<usize>> = HashMap::new();
    for (i, c) in carts.iter().enumerate() {
        twins
            .entry((c.platform, clean_label(&c.stem).to_lowercase()))
            .or_default()
            .push(i);
    }
    for group in twins.values().filter(|g| g.len() > 1) {
        let all: Vec<Vec<String>> = group.iter().map(|&i| label_tags(&carts[i].stem)).collect();
        for (&i, tags) in group.iter().zip(&all) {
            carts[i].tags = tags
                .iter()
                .filter(|t| !all.iter().all(|other| other.contains(t)))
                .cloned()
                .collect();
        }
    }
}

pub fn listing(dir: &Path) -> HashMap<String, String> {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return HashMap::new();
    };
    entries
        .filter_map(|e| e.ok()?.file_name().into_string().ok())
        .filter(|name| !name.starts_with('.'))
        .map(|name| (name.to_lowercase(), name))
        .collect()
}

fn is_file(entry: &std::fs::DirEntry, complete: &mut bool) -> bool {
    match entry.file_type() {
        Ok(t) if t.is_symlink() => match entry.path().metadata() {
            Ok(meta) => meta.is_file(),
            Err(_) => {
                *complete = false;
                false
            }
        },
        Ok(t) => t.is_file(),
        Err(_) => {
            *complete = false;
            false
        }
    }
}

pub fn sort_key(stem: &str) -> (u8, String) {
    (group_of(stem), stem.to_uppercase())
}

fn group_of(stem: &str) -> u8 {
    match stem.chars().find(|c| !c.is_whitespace()) {
        Some(c) if c.is_ascii_digit() => 0,
        Some(c) if c.is_alphabetic() => 1,
        _ => 2,
    }
}

pub fn initial(stem: &str) -> char {
    match group_of(stem) {
        1 => stem
            .chars()
            .find(|c| !c.is_whitespace())
            .and_then(|c| c.to_uppercase().next())
            .unwrap_or('#'),
        _ => '#',
    }
}

pub fn is_hidden(p: &Path) -> bool {
    p.file_name()
        .is_some_and(|n| n.as_encoded_bytes().starts_with(b"."))
}

pub(crate) fn decode_filename(name: &OsStr) -> Option<String> {
    #[cfg(unix)]
    let bytes = {
        use std::os::unix::ffi::OsStrExt;
        name.as_bytes()
    };
    #[cfg(not(unix))]
    let bytes = name.to_str()?.as_bytes();
    let decoded = match std::str::from_utf8(bytes) {
        Ok(name) => name.to_string(),
        Err(_) => encoding_rs::GB18030
            .decode_without_bom_handling_and_without_replacement(bytes)?
            .into_owned(),
    };
    Some(recover_utf8_mojibake(&decoded).unwrap_or(decoded))
}

pub fn find_file_by_stem(dir: &Path, stem: &str, extension: &str) -> Option<PathBuf> {
    let direct = dir.join(format!("{stem}.{extension}"));
    if direct.is_file() {
        return Some(direct);
    }
    std::fs::read_dir(dir)
        .ok()?
        .filter_map(Result::ok)
        .map(|entry| entry.path())
        .find(|path| {
            !is_hidden(path)
                && path.is_file()
                && path
                    .extension()
                    .and_then(|name| name.to_str())
                    .is_some_and(|candidate| candidate.eq_ignore_ascii_case(extension))
                && path
                    .file_stem()
                    .and_then(decode_filename)
                    .is_some_and(|candidate| candidate == stem)
        })
}

pub(crate) fn find_dir_by_name(dir: &Path, name: &str) -> Option<PathBuf> {
    let direct = dir.join(name);
    if direct.is_dir() {
        return Some(direct);
    }
    std::fs::read_dir(dir)
        .ok()?
        .filter_map(Result::ok)
        .map(|entry| entry.path())
        .find(|path| {
            !is_hidden(path)
                && path.is_dir()
                && path
                    .file_name()
                    .and_then(decode_filename)
                    .is_some_and(|candidate| candidate == name)
        })
}

fn recover_utf8_mojibake(name: &str) -> Option<String> {
    if name.chars().any(is_cjk) {
        return None;
    }
    let bytes: Option<Vec<u8>> = name
        .chars()
        .map(|character| u8::try_from(character as u32).ok())
        .collect();
    let recovered = String::from_utf8(bytes?).ok()?;
    recovered.chars().any(is_cjk).then_some(recovered)
}

fn is_cjk(character: char) -> bool {
    matches!(
        character,
        '\u{3400}'..='\u{4dbf}'
            | '\u{4e00}'..='\u{9fff}'
            | '\u{f900}'..='\u{faff}'
            | '\u{20000}'..='\u{2fa1f}'
    )
}

pub fn clean_label(stem: &str) -> String {
    let mut bare = String::with_capacity(stem.len());
    let mut depth = 0u32;
    for ch in stem.chars() {
        match ch {
            '(' | '[' => depth += 1,
            ')' | ']' => depth = depth.saturating_sub(1),
            _ if depth == 0 => bare.push(ch),
            _ => {}
        }
    }

    let mut out = String::with_capacity(bare.len());
    for word in bare.split_whitespace().filter(|w| *w != "-") {
        if !out.is_empty() {
            out.push(' ');
        }
        out.push_str(word);
    }
    if out.is_empty() {
        stem.to_string()
    } else {
        out
    }
}

pub fn label_tags(stem: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut depth = 0u32;
    let mut cur = String::new();
    for ch in stem.chars() {
        match ch {
            '(' | '[' => {
                depth += 1;
                if depth == 1 {
                    cur.clear();
                    continue;
                }
            }
            ')' | ']' => {
                depth = depth.saturating_sub(1);
                if depth == 0 {
                    let t = cur.trim();
                    if !t.is_empty() {
                        out.push(t.to_string());
                    }
                    continue;
                }
            }
            _ => {}
        }
        if depth >= 1 {
            cur.push(ch);
        }
    }
    out
}
