// Blank cells, whether erased, inserted or scrolled in, take the current
// background colour and nothing else, as in xterm (back colour erase) and
// most other terminals: an underline or inverse video must not stretch
// across cleared space, and a background must.

const RED: vt100::Color = vt100::Color::Idx(1);
const GREEN: vt100::Color = vt100::Color::Idx(2);

// a 5x10 screen with every attribute on: bold, italic, underline, inverse,
// red on green
fn attributed() -> vt100::Parser {
    let mut parser = vt100::Parser::new(nz(5), nz(10), 10);
    parser.process(b"\x1b[1;3;4;7;31;42m");
    parser
}

// written with every attribute on, and untouched since
fn assert_text(screen: &vt100::Screen, row: u16, col: u16, text: &str) {
    let cell = screen.cell(row, col).unwrap();
    assert_eq!(cell.contents(), text, "({row}, {col})");
    assert_eq!(cell.fgcolor(), RED, "({row}, {col})");
    assert_eq!(cell.bgcolor(), GREEN, "({row}, {col})");
    assert!(cell.bold(), "({row}, {col})");
    assert!(cell.italic(), "({row}, {col})");
    assert!(cell.underline(), "({row}, {col})");
    assert!(cell.inverse(), "({row}, {col})");
}

// blanked while every attribute was on: only the background remains
fn assert_blank(screen: &vt100::Screen, row: u16, col: u16) {
    let cell = screen.cell(row, col).unwrap();
    assert_eq!(cell.contents(), "", "({row}, {col})");
    assert_eq!(cell.fgcolor(), vt100::Color::Default, "({row}, {col})");
    assert_eq!(cell.bgcolor(), GREEN, "({row}, {col})");
    assert!(!cell.bold(), "({row}, {col})");
    assert!(!cell.dim(), "({row}, {col})");
    assert!(!cell.italic(), "({row}, {col})");
    assert!(!cell.underline(), "({row}, {col})");
    assert!(!cell.inverse(), "({row}, {col})");
}

// never written or blanked
fn assert_untouched(screen: &vt100::Screen, row: u16, col: u16) {
    let cell = screen.cell(row, col).unwrap();
    assert_eq!(cell.contents(), "", "({row}, {col})");
    assert_eq!(cell.bgcolor(), vt100::Color::Default, "({row}, {col})");
    assert!(!cell.underline(), "({row}, {col})");
}

fn assert_blank_row(screen: &vt100::Screen, row: u16) {
    for col in 0..10 {
        assert_blank(screen, row, col);
    }
}

fn nz(n: u16) -> std::num::NonZeroU16 {
    std::num::NonZeroU16::new(n).unwrap()
}

#[test]
fn underline_does_not_cross_cleared_space() {
    let mut parser = vt100::Parser::new(nz(5), nz(10), 0);
    parser.process(b"\x1b[4mabc\x1b[K");
    let screen = parser.screen();
    for col in 0..3 {
        assert!(screen.cell(0, col).unwrap().underline());
    }
    for col in 3..10 {
        let cell = screen.cell(0, col).unwrap();
        assert!(!cell.has_contents());
        assert!(!cell.underline(), "column {col} is underlined");
        assert_eq!(cell.bgcolor(), vt100::Color::Default);
    }
}

#[test]
fn el() {
    let mut parser = attributed();
    parser.process(b"abcdef\x1b[3G\x1b[K");
    let screen = parser.screen();
    assert_text(screen, 0, 0, "a");
    assert_text(screen, 0, 1, "b");
    for col in 2..10 {
        assert_blank(screen, 0, col);
    }

    let mut parser = attributed();
    parser.process(b"abcdef\x1b[3G\x1b[1K");
    let screen = parser.screen();
    for col in 0..3 {
        assert_blank(screen, 0, col);
    }
    assert_text(screen, 0, 3, "d");
    assert_text(screen, 0, 5, "f");
    assert_untouched(screen, 0, 6);

    let mut parser = attributed();
    parser.process(b"abcdef\x1b[2K");
    assert_blank_row(parser.screen(), 0);
}

#[test]
fn ed() {
    let mut parser = attributed();
    parser.process(b"abcdef\r\nghijkl\x1b[2;3H\x1b[J");
    let screen = parser.screen();
    assert_text(screen, 0, 0, "a");
    assert_text(screen, 0, 5, "f");
    assert_untouched(screen, 0, 6);
    assert_text(screen, 1, 0, "g");
    assert_text(screen, 1, 1, "h");
    for col in 2..10 {
        assert_blank(screen, 1, col);
    }
    for row in 2..5 {
        assert_blank_row(screen, row);
    }

    let mut parser = attributed();
    parser.process(b"abcdef\r\nghijkl\x1b[2;3H\x1b[1J");
    let screen = parser.screen();
    assert_blank_row(screen, 0);
    for col in 0..3 {
        assert_blank(screen, 1, col);
    }
    assert_text(screen, 1, 3, "j");
    assert_text(screen, 1, 5, "l");
    assert_untouched(screen, 1, 6);
    assert_untouched(screen, 2, 0);

    let mut parser = attributed();
    parser.process(b"abcdef\r\nghijkl\x1b[2J");
    for row in 0..5 {
        assert_blank_row(parser.screen(), row);
    }
}

#[test]
fn ech() {
    let mut parser = attributed();
    parser.process(b"abcdef\x1b[2G\x1b[3X");
    let screen = parser.screen();
    assert_text(screen, 0, 0, "a");
    for col in 1..4 {
        assert_blank(screen, 0, col);
    }
    assert_text(screen, 0, 4, "e");
    assert_text(screen, 0, 5, "f");
    assert_untouched(screen, 0, 6);
}

#[test]
fn ich_and_dch() {
    let mut parser = attributed();
    parser.process(b"abcdef\x1b[2G\x1b[2@");
    let screen = parser.screen();
    assert_text(screen, 0, 0, "a");
    assert_blank(screen, 0, 1);
    assert_blank(screen, 0, 2);
    assert_text(screen, 0, 3, "b");
    assert_text(screen, 0, 7, "f");
    assert_untouched(screen, 0, 8);

    let mut parser = attributed();
    parser.process(b"abcdef\x1b[2G\x1b[2P");
    let screen = parser.screen();
    assert_text(screen, 0, 0, "a");
    assert_text(screen, 0, 1, "d");
    assert_text(screen, 0, 3, "f");
    assert_untouched(screen, 0, 4);
    assert_untouched(screen, 0, 7);
    assert_blank(screen, 0, 8);
    assert_blank(screen, 0, 9);
}

#[test]
fn il_and_dl() {
    let mut parser = attributed();
    parser.process(b"abc\r\ndef\r\nghi\x1b[2;1H\x1b[L");
    let screen = parser.screen();
    assert_text(screen, 0, 0, "a");
    assert_blank_row(screen, 1);
    assert_text(screen, 2, 0, "d");
    assert_text(screen, 3, 0, "g");
    assert_untouched(screen, 4, 0);

    let mut parser = attributed();
    parser.process(b"abc\r\ndef\r\nghi\x1b[2;1H\x1b[M");
    let screen = parser.screen();
    assert_text(screen, 0, 0, "a");
    assert_text(screen, 1, 0, "g");
    assert_untouched(screen, 2, 0);
    assert_untouched(screen, 3, 0);
    assert_blank_row(screen, 4);
}

#[test]
fn scrolling() {
    // SU and SD
    let mut parser = attributed();
    parser.process(b"abc\r\ndef\x1b[S");
    let screen = parser.screen();
    assert_text(screen, 0, 0, "d");
    assert_untouched(screen, 1, 0);
    assert_blank_row(screen, 4);

    let mut parser = attributed();
    parser.process(b"abc\r\ndef\x1b[T");
    let screen = parser.screen();
    assert_blank_row(screen, 0);
    assert_text(screen, 1, 0, "a");
    assert_text(screen, 2, 0, "d");

    // a line feed on the last row
    let mut parser = attributed();
    parser.process(b"a\r\nb\r\nc\r\nd\r\ne\r\n");
    let screen = parser.screen();
    assert_text(screen, 0, 0, "b");
    assert_text(screen, 3, 0, "e");
    assert_blank_row(screen, 4);
    assert_eq!(screen.cursor_position(), (4, 0));

    // a reverse index on the first row
    let mut parser = attributed();
    parser.process(b"abc\x1bM");
    let screen = parser.screen();
    assert_blank_row(screen, 0);
    assert_text(screen, 1, 0, "a");

    // wrapping off the last row
    let mut parser = attributed();
    parser.process(b"\x1b[5;1Habcdefghijk");
    let screen = parser.screen();
    assert_text(screen, 3, 0, "a");
    assert_text(screen, 3, 9, "j");
    assert_text(screen, 4, 0, "k");
    for col in 1..10 {
        assert_blank(screen, 4, col);
    }

    // inside a scroll region
    let mut parser = attributed();
    parser.process(b"\x1b[2;3r\x1b[3;1Habc\r\n");
    let screen = parser.screen();
    assert_untouched(screen, 0, 0);
    assert_text(screen, 1, 0, "a");
    assert_blank_row(screen, 2);
    assert_untouched(screen, 3, 0);
}

#[test]
fn a_wide_character_half_overwritten() {
    let mut parser = attributed();
    parser.process("日本\x1b[2Gx".as_bytes());
    let screen = parser.screen();
    // the orphaned left half is blank, with the background only
    assert_blank(screen, 0, 0);
    assert_text(screen, 0, 1, "x");
    assert_text(screen, 0, 2, "本");
}
