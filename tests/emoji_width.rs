// Regression tests for emoji-presentation width: a base char + U+FE0F
// (VS16) is two columns, but vt100 used to store it as one narrow cell,
// shifting everything after it and leaving on-screen residue.
mod helpers;

fn cell(parser: &vt100::Parser, row: u16, col: u16) -> (String, bool, bool) {
    let screen = parser.screen();
    let c = screen.cell(row, col).unwrap();
    (
        c.contents().to_string(),
        c.is_wide(),
        c.is_wide_continuation(),
    )
}

#[test]
fn vs16_emoji_occupies_two_columns() {
    let mut parser = helpers::new(1, 20, 0);
    parser.process("A\u{2764}\u{FE0F}B".as_bytes());

    assert_eq!(cell(&parser, 0, 0), ("A".into(), false, false), "col0 = A");
    assert_eq!(
        cell(&parser, 0, 1),
        ("\u{2764}\u{FE0F}".into(), true, false),
        "col1 = ❤️ and must be WIDE"
    );
    assert_eq!(
        cell(&parser, 0, 2),
        (String::new(), false, true),
        "col2 must be a wide continuation"
    );
    assert_eq!(
        cell(&parser, 0, 3),
        ("B".into(), false, false),
        "col3 = B (not shifted)"
    );
    assert_eq!(parser.screen().cursor_position(), (0, 4));
}

#[test]
fn vs16_promotion_over_existing_wide_char_does_not_orphan_continuation() {
    let mut parser = helpers::new(1, 6, 0);
    parser.process("a\u{4E2D}".as_bytes());
    // Move the cursor back to col0 and write a text-presentation base + VS16.
    // The base lands on col0; its continuation lands on col1, which is the
    // first half of 中 — so promotion must also clear 中's now-stranded
    // continuation at col2 instead of leaving it orphaned.
    parser.process(b"\x1b[1;1H");
    parser.process("\u{2764}\u{FE0F}".as_bytes());

    assert_eq!(
        cell(&parser, 0, 0),
        ("\u{2764}\u{FE0F}".into(), true, false),
        "col0 = ❤️ wide"
    );
    assert_eq!(
        cell(&parser, 0, 1),
        ("".into(), false, true),
        "col1 = ❤️ continuation"
    );
    assert_eq!(
        cell(&parser, 0, 2),
        ("".into(), false, false),
        "col2 (clobbered 中's old continuation) must be cleared, not orphaned"
    );
}

#[test]
fn vs16_emoji_wrapping() {
    let mut parser = vt100::Parser::default();
    parser.process(b"\x1b[2H");
    parser.process("b".repeat(79).as_bytes());
    parser.process(b"\x1b[H");
    parser.process("a".repeat(79).as_bytes());
    assert_eq!(cell(&parser, 0, 0), ("a".into(), false, false));
    assert_eq!(cell(&parser, 0, 78), ("a".into(), false, false));
    assert_eq!(cell(&parser, 0, 79), ("".into(), false, false));
    assert_eq!(cell(&parser, 1, 0), ("b".into(), false, false));
    assert_eq!(cell(&parser, 1, 1), ("b".into(), false, false));
    assert_eq!(cell(&parser, 1, 2), ("b".into(), false, false));
    assert_eq!(parser.screen().cursor_position(), (0, 79));

    parser.process("\u{2764}".as_bytes());
    assert_eq!(cell(&parser, 0, 78), ("a".into(), false, false));
    assert_eq!(cell(&parser, 0, 79), ("\u{2764}".into(), false, false));
    assert_eq!(cell(&parser, 1, 0), ("b".into(), false, false));
    assert_eq!(cell(&parser, 1, 1), ("b".into(), false, false));
    assert_eq!(cell(&parser, 1, 2), ("b".into(), false, false));
    assert_eq!(parser.screen().cursor_position(), (0, 80));

    parser.process("\u{FE0F}".as_bytes());
    assert_eq!(cell(&parser, 0, 78), ("a".into(), false, false));
    assert_eq!(cell(&parser, 0, 79), ("".into(), false, false));
    assert_eq!(
        cell(&parser, 1, 0),
        ("\u{2764}\u{FE0F}".into(), true, false)
    );
    assert_eq!(cell(&parser, 1, 1), ("".into(), false, true));
    assert_eq!(cell(&parser, 1, 2), ("b".into(), false, false));
    assert_eq!(parser.screen().cursor_position(), (1, 2));
}

// Widened in the bottom-right cell, the emoji wraps onto a new row and the
// screen scrolls, so the base cell it leaves behind is a row higher than
// where it was written. Clearing the old position instead left a wide cell
// with no continuation in the last column of that row, and the next write
// there panicked looking for the continuation.
#[test]
fn vs16_emoji_widened_in_the_bottom_right_cell() {
    for then in [
        "\x1b[23;80He",
        "\x1b[23;79H\u{26a0}\u{fe0f}",
        "\x1b[23;79H  ",
    ] {
        let mut parser = vt100::Parser::default();
        parser.process("\x1b[24;79H \u{26a0}\u{fe0f}".as_bytes());
        assert_eq!(cell(&parser, 22, 78), (" ".into(), false, false));
        assert_eq!(cell(&parser, 22, 79), ("".into(), false, false));
        assert_eq!(
            cell(&parser, 23, 0),
            ("\u{26a0}\u{fe0f}".into(), true, false)
        );
        assert_eq!(cell(&parser, 23, 1), ("".into(), false, true));
        assert_eq!(parser.screen().cursor_position(), (23, 2));

        parser.process(then.as_bytes());
    }
}

// With one row, the row the emoji was written in scrolls away entirely.
#[test]
fn vs16_emoji_widened_in_the_last_cell_of_a_one_row_screen() {
    let mut parser = helpers::new(1, 4, 0);
    parser.process("abc\u{26a0}\u{fe0f}".as_bytes());
    assert_eq!(
        cell(&parser, 0, 0),
        ("\u{26a0}\u{fe0f}".into(), true, false)
    );
    assert_eq!(cell(&parser, 0, 1), ("".into(), false, true));
    assert_eq!(parser.screen().cursor_position(), (0, 2));
    parser.process(b"de");
    assert_eq!(parser.screen().contents(), "\u{26a0}\u{fe0f}de");
}
