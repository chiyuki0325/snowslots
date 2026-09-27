#[derive(Copy, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Debug)]
pub enum Platform {
    Gba,
}

impl Platform {
    pub const ALL: [Platform; 1] = [Platform::Gba];

    pub const fn dir_name(self) -> &'static str {
        match self {
            Platform::Gba => "GBA",
        }
    }

    pub const fn display_name(self) -> &'static str {
        match self {
            Platform::Gba => "Game Boy Advance",
        }
    }

    pub(crate) const fn code(self) -> u8 {
        match self {
            Platform::Gba => 0,
        }
    }

    pub(crate) const fn from_code(code: u8) -> Option<Self> {
        match code {
            0 => Some(Platform::Gba),
            _ => None,
        }
    }
}
