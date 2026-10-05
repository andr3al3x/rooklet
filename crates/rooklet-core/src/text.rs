//! Sanitize untrusted text before terminal rendering.
pub fn clean(text: &str) -> String {
    text.chars().filter(|c| safe_character(*c)).collect()
}
/// Human diagnostics may contain deliberate line breaks, but no terminal controls.
pub fn clean_multiline(text: &str) -> String {
    text.chars()
        .filter(|c| *c == '\n' || safe_character(*c))
        .collect()
}
fn safe_character(c: char) -> bool {
    !c.is_control()
        && !matches!(c, '\u{061c}' | '\u{200e}'..='\u{200f}' | '\u{202a}'..='\u{202e}' | '\u{2066}'..='\u{206f}')
}
