use rooklet_core::text::{clean, clean_multiline};

#[test]
fn sanitizers_remove_terminal_and_bidi_controls_and_preserve_unicode() {
    for (input, single_line, multiline) in [
        (
            "hello\x1b[31m\r\n\u{202e}app",
            "hello[31mapp",
            "hello[31m\napp",
        ),
        (
            "first\r\nsecond\t\x1b[2J\u{061c}\u{200e}\u{200f}\u{202e}\u{2066}\u{206f}last",
            "firstsecond[2Jlast",
            "first\nsecond[2Jlast",
        ),
        (
            "Café λ 日本語 🦉\nالتالي",
            "Café λ 日本語 🦉التالي",
            "Café λ 日本語 🦉\nالتالي",
        ),
    ] {
        assert_eq!(clean(input), single_line);
        assert_eq!(clean_multiline(input), multiline);
    }
}
