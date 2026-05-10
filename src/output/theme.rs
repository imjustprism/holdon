pub(crate) const ARROW: &str = "→";

pub(crate) const TEE: &str = "├";
pub(crate) const ELL: &str = "└";

pub(crate) const CHECK: &str = "✓";
pub(crate) const CROSS: &str = "✗";

pub(crate) const ELLIPSIS: &str = "…";
pub(crate) const SEP: &str = "·";
pub(crate) const DOLLAR: &str = "$";

pub(crate) const SPINNER: &[&str] = &["⠋", "⠙", "⠹", "⠸", "⠼", "⠴", "⠦", "⠧", "⠇", "⠏"];
pub(crate) const SPINNER_INTERVAL_MS: u64 = 80;

pub(crate) const SPARK: &[char] = &['▁', '▂', '▃', '▄', '▅', '▆', '▇', '█'];

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[allow(dead_code)]
pub(crate) enum Color {
    Waiting,
    Ready,
    Failed,
    Text,
    Subtle,
}

impl Color {
    pub(crate) const fn rgb(self) -> (u8, u8, u8) {
        match self {
            Self::Waiting => (0xff, 0x9b, 0x3d),
            Self::Ready => (0x6c, 0xe0, 0xa0),
            Self::Failed => (0xff, 0x6b, 0x6b),
            Self::Text => (0xe7, 0xea, 0xf0),
            Self::Subtle => (0x7c, 0x85, 0x97),
        }
    }

    pub(crate) const fn bold(self) -> bool {
        matches!(self, Self::Waiting | Self::Ready | Self::Failed)
    }
}
