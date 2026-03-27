//! Helpers for interpreting channel-level `podcast:medium` values.

pub const MUSIC: &str = "music";
pub const PUBLISHER: &str = "publisher";
pub const MUSICL: &str = "musicL";

#[must_use]
pub fn is_music(raw_medium: Option<&str>) -> bool {
    raw_medium.is_some_and(|medium| medium.eq_ignore_ascii_case(MUSIC))
}

#[must_use]
pub fn is_publisher(raw_medium: Option<&str>) -> bool {
    raw_medium.is_some_and(|medium| medium.eq_ignore_ascii_case(PUBLISHER))
}

#[must_use]
pub fn is_musicl(raw_medium: Option<&str>) -> bool {
    raw_medium.is_some_and(|medium| medium.eq_ignore_ascii_case(MUSICL))
}

#[must_use]
pub fn resolver_excluded(raw_medium: Option<&str>) -> bool {
    is_musicl(raw_medium)
}

#[must_use]
pub fn payment_exempt(raw_medium: Option<&str>) -> bool {
    is_publisher(raw_medium) || is_musicl(raw_medium)
}
