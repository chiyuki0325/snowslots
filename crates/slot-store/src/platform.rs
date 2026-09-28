use std::path::Path;

#[derive(Copy, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Debug)]
pub enum Platform {
    Gba,
    Gb,
    Gbc,
}

impl Platform {
    pub const ALL: [Platform; 3] = [Platform::Gba, Platform::Gb, Platform::Gbc];

    pub const fn dir_name(self) -> &'static str {
        match self {
            Platform::Gba => "GBA",
            Platform::Gb => "GB",
            Platform::Gbc => "GBC",
        }
    }

    pub const fn display_name(self) -> &'static str {
        match self {
            Platform::Gba => "Game Boy Advance",
            Platform::Gb => "Game Boy",
            Platform::Gbc => "Game Boy Color",
        }
    }

    pub fn accepts(self, path: &Path) -> bool {
        path.extension()
            .and_then(|e| e.to_str())
            .is_some_and(|ext| match self {
                Platform::Gba => ext.eq_ignore_ascii_case("gba"),
                Platform::Gb | Platform::Gbc => {
                    ext.eq_ignore_ascii_case("gb") || ext.eq_ignore_ascii_case("gbc")
                }
            })
    }

    pub const fn picture(self) -> (u32, u32) {
        match self {
            Platform::Gba => (240, 160),
            Platform::Gb | Platform::Gbc => (160, 144),
        }
    }

    pub(crate) const fn code(self) -> u8 {
        match self {
            Platform::Gba => 0,
            Platform::Gb => 1,
            Platform::Gbc => 2,
        }
    }

    pub(crate) const fn from_code(code: u8) -> Option<Self> {
        match code {
            0 => Some(Platform::Gba),
            1 => Some(Platform::Gb),
            2 => Some(Platform::Gbc),
            _ => None,
        }
    }
}
