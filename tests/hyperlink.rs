fn uri(screen: &vt100::Screen, row: u16, col: u16) -> Option<&str> {
    screen.cell_hyperlink(row, col).map(vt100::Hyperlink::uri)
}

#[test]
fn basic() {
    let mut parser = vt100::Parser::default();
    parser
        .process(b"a \x1b]8;;https://example.com/\x1b\\link\x1b]8;;\x1b\\ b");
    assert_eq!(parser.screen().contents(), "a link b");
    assert_eq!(uri(parser.screen(), 0, 1), None);
    for col in 2..6 {
        assert_eq!(
            uri(parser.screen(), 0, col),
            Some("https://example.com/")
        );
    }
    assert_eq!(uri(parser.screen(), 0, 6), None);
    assert_eq!(uri(parser.screen(), 0, 7), None);
    assert_eq!(parser.screen().cell_hyperlink(0, 2).unwrap().id(), None);
    assert!(parser.screen().hyperlink().is_none());

    // bel-terminated, and with an id
    parser.process(b"\r\n\x1b]8;id=x1;https://example.com/2\x07two");
    let link = parser.screen().cell_hyperlink(1, 0).unwrap();
    assert_eq!(link.uri(), "https://example.com/2");
    assert_eq!(link.id(), Some("x1"));
    assert_eq!(parser.screen().hyperlink(), Some(link));
}

#[test]
fn uri_with_semicolons() {
    let mut parser = vt100::Parser::default();
    parser.process(
        b"\x1b]8;;https://example.com/?x=1;y=2\x1b\\link\x1b]8;;\x1b\\",
    );
    assert_eq!(
        uri(parser.screen(), 0, 0),
        Some("https://example.com/?x=1;y=2")
    );
}

#[test]
fn claude_code() {
    // as printed by claude code 2.1: an id, and the cursor moved over the
    // space after the text before the link is closed
    let mut parser = vt100::Parser::new(5, 60, 0);
    parser.process(b"Here is \x1b]8;id=azmyb0;https://example.com/artifact/abc123\x07\x1b[94mthe artifact\x1b[22G\x1b]8;;\x07\x1b[39mand more");
    assert_eq!(parser.screen().contents(), "Here is the artifact and more");
    assert_eq!(uri(parser.screen(), 0, 7), None);
    for col in 8..20 {
        let link = parser.screen().cell_hyperlink(0, col).unwrap();
        assert_eq!(link.uri(), "https://example.com/artifact/abc123");
        assert_eq!(link.id(), Some("azmyb0"));
    }
    assert_eq!(uri(parser.screen(), 0, 20), None);
    assert_eq!(uri(parser.screen(), 0, 21), None);
}

#[test]
fn claude_code_across_rows() {
    // claude code closes a link at the end of a row and opens it again,
    // with the same id, on the next
    let url = "https://example.com/a/very/long/path?query=1";
    let open = format!("\x1b]8;id=1orb7vg;{url}\x07");
    let mut parser = vt100::Parser::new(10, 30, 0);
    parser.process(
        format!(
            "● See {open}https://example.com/a/ve\x1b]8;;\x07\r\x1b[2C\x1b[1B{open}ry/long/path?query=1\x1b]8;;\x07 now."
        )
        .as_bytes(),
    );
    let screen = parser.screen();
    assert_eq!(uri(screen, 0, 5), None);
    assert_eq!(uri(screen, 1, 1), None);
    assert_eq!(uri(screen, 1, 22), None);
    let first = screen.cell_hyperlink(0, 6).unwrap();
    for (row, col) in [(0, 29), (1, 2), (1, 20)] {
        assert_eq!(screen.cell_hyperlink(row, col), Some(first));
    }
    assert_eq!(first.uri(), url);
    assert_eq!(first.id(), Some("1orb7vg"));
}

#[test]
fn codex_across_rows() {
    // codex 0.162 keeps one link open across rows, moving the cursor to
    // each row's indent
    let url = "https://example.com/a/very/long/path?query=2";
    let mut parser = vt100::Parser::new(10, 30, 0);
    parser.process(
        format!(
            "\x1b[1;1H• See \x1b]8;;{url}\x07https://\x1b[2;3Hexample.com/a/very/\x1b[3;3Hlong/path?query=2\x1b]8;;\x07 now."
        )
        .as_bytes(),
    );
    let screen = parser.screen();
    assert_eq!(
        screen.contents(),
        "• See https://\n  example.com/a/very/\n  long/path?query=2 now."
    );
    for (row, col) in [(0, 6), (0, 13), (1, 2), (1, 20), (2, 2), (2, 18)] {
        assert_eq!(uri(screen, row, col), Some(url), "at {row},{col}");
    }
    // the cursor moved over the indent, so it isn't linked
    for (row, col) in [(0, 5), (1, 1), (2, 1), (2, 19), (2, 20)] {
        assert_eq!(uri(screen, row, col), None, "at {row},{col}");
    }
}

#[test]
fn codex_label_and_url() {
    // codex 0.162 without a known terminal: "text (url)", both linked
    let url = "https://example.com/artifact/abc123";
    let mut parser = vt100::Parser::new(10, 80, 0);
    parser.process(
        format!(
            "\x1b[?1049h\x1b[10;11H\x1b[4m\x1b]8;;{url}\x07the artifact\x1b]8;;\x07\x1b[24m (\x1b[4m\x1b]8;;{url}\x07{url}\x1b]8;;\x07\x1b[24m)"
        )
        .as_bytes(),
    );
    let screen = parser.screen();
    assert_eq!(uri(screen, 9, 10), Some(url));
    assert_eq!(uri(screen, 9, 22), None);
    assert_eq!(uri(screen, 9, 23), None);
    assert_eq!(uri(screen, 9, 24), Some(url));
    assert_eq!(screen.cell_hyperlink(9, 10), screen.cell_hyperlink(9, 24));
}

#[test]
fn autowrap() {
    let mut parser = vt100::Parser::new(5, 10, 0);
    parser.process(
        b"abcdef\x1b]8;;https://example.com/\x070123456789\x1b]8;;\x07 after",
    );
    let screen = parser.screen();
    assert_eq!(uri(screen, 0, 5), None);
    assert_eq!(uri(screen, 0, 6), Some("https://example.com/"));
    assert_eq!(uri(screen, 1, 5), Some("https://example.com/"));
    assert_eq!(uri(screen, 1, 6), None);
}

#[test]
fn nested_open_replaces() {
    let mut parser = vt100::Parser::default();
    parser.process(b"\x1b]8;;https://a.example/\x07one \x1b]8;id=2;https://b.example/\x07two\x1b]8;;\x07 three");
    let screen = parser.screen();
    assert_eq!(uri(screen, 0, 1), Some("https://a.example/"));
    assert_eq!(uri(screen, 0, 5), Some("https://b.example/"));
    assert_eq!(uri(screen, 0, 9), None);
}

#[test]
fn sgr_reset_keeps_link() {
    let mut parser = vt100::Parser::default();
    parser.process(
        b"\x1b]8;;https://example.com/\x07\x1b[1ma\x1b[mb\x1b]8;;\x07c",
    );
    let screen = parser.screen();
    assert_eq!(uri(screen, 0, 0), Some("https://example.com/"));
    assert_eq!(uri(screen, 0, 1), Some("https://example.com/"));
    assert_eq!(uri(screen, 0, 2), None);
}

#[test]
fn overwrite_and_erase() {
    let mut parser = vt100::Parser::default();
    parser
        .process(b"\x1b]8;;https://example.com/\x07linked text\x1b]8;;\x07");
    parser.process(b"\x1b[1;1Hplain");
    let screen = parser.screen();
    assert_eq!(uri(screen, 0, 4), None);
    assert_eq!(uri(screen, 0, 5), Some("https://example.com/"));

    // erasing doesn't link the cells, even with a link open
    parser.process(b"\x1b]8;;https://example.com/2\x07\x1b[1;1H\x1b[2K");
    parser.process(b"\x1b[2;1H\x1b[X\x1b[3;1H\x1b[J");
    let screen = parser.screen();
    assert_eq!(screen.contents(), "");
    for row in 0..3 {
        for col in 0..10 {
            assert_eq!(uri(screen, row, col), None);
        }
    }
}

#[test]
fn wide() {
    let mut parser = vt100::Parser::default();
    parser.process(
        "\x1b]8;;https://example.com/\x07日本\x1b]8;;\x07語".as_bytes(),
    );
    let screen = parser.screen();
    for col in 0..4 {
        assert_eq!(uri(screen, 0, col), Some("https://example.com/"));
    }
    assert_eq!(uri(screen, 0, 4), None);
    assert_eq!(uri(screen, 0, 5), None);

    // overwriting half of a wide character clears the other half
    parser.process(b"\x1b[1;2Hx");
    let screen = parser.screen();
    assert_eq!(uri(screen, 0, 0), None);
    assert_eq!(uri(screen, 0, 1), None);
}

#[test]
fn scrollback() {
    let mut parser = vt100::Parser::new(3, 20, 10);
    parser.process(
        b"\x1b]8;;https://example.com/old\x07old link\x1b]8;;\x07\r\n",
    );
    for i in 0..5 {
        parser.process(format!("line {i}\r\n").as_bytes());
    }
    assert_eq!(parser.screen().contents(), "line 3\nline 4");
    parser.screen_mut().set_scrollback(4);
    assert_eq!(parser.screen().contents(), "old link\nline 0\nline 1");
    assert_eq!(uri(parser.screen(), 0, 0), Some("https://example.com/old"));
    assert_eq!(uri(parser.screen(), 1, 0), None);
}

#[test]
fn alternate_screen() {
    let mut parser = vt100::Parser::new(5, 40, 0);
    parser.process(
        b"\x1b]8;;https://example.com/normal\x07normal\x1b]8;;\x07\r\n",
    );
    parser.process(b"\x1b[?1049h\x1b[3;5H\x1b]8;;https://example.com/alt\x07alt\x1b]8;;\x07");
    assert_eq!(uri(parser.screen(), 2, 4), Some("https://example.com/alt"));
    assert_eq!(uri(parser.screen(), 0, 0), None);
    parser.process(b"\x1b[?1049l");
    assert_eq!(
        uri(parser.screen(), 0, 0),
        Some("https://example.com/normal")
    );
    assert_eq!(uri(parser.screen(), 2, 4), None);
}

#[test]
fn reset() {
    let mut parser = vt100::Parser::default();
    parser.process(b"\x1b]8;;https://example.com/\x07a\x1bcb");
    assert_eq!(parser.screen().contents(), "b");
    assert_eq!(uri(parser.screen(), 0, 0), None);
}

#[test]
fn malformed_and_too_long() {
    #[derive(Default)]
    struct Unhandled(Vec<Vec<Vec<u8>>>);
    impl vt100::Callbacks for Unhandled {
        fn unhandled_osc(&mut self, _: &mut vt100::Screen, params: &[&[u8]]) {
            self.0.push(params.iter().map(|p| p.to_vec()).collect());
        }
    }
    let mut parser =
        vt100::Parser::new_with_callbacks(24, 80, 0, Unhandled::default());
    parser.process(b"\x1b]8\x07\x1b]8;https://example.com/\x07a");
    assert_eq!(
        parser.callbacks().0,
        [
            vec![b"8".to_vec()],
            vec![b"8".to_vec(), b"https://example.com/".to_vec()]
        ]
    );
    assert_eq!(uri(parser.screen(), 0, 0), None);

    // a uri too long to keep ends the link before it
    let long = format!("https://example.com/{}", "a".repeat(8192));
    parser.process(
        format!("\x1b]8;;https://example.com/\x07b\x1b]8;;{long}\x07c")
            .as_bytes(),
    );
    assert_eq!(uri(parser.screen(), 0, 1), Some("https://example.com/"));
    assert_eq!(uri(parser.screen(), 0, 2), None);
}

#[test]
fn many_links() {
    // more distinct links than there are ids: the ones no cell has any more
    // are forgotten to make room
    let mut parser = vt100::Parser::new(2, 40, 0);
    parser
        .process(b"\x1b]8;;https://example.com/kept\x07kept\x1b]8;;\x07\r\n");
    for i in 0..70_000 {
        parser.process(
            format!("\r\x1b]8;;https://example.com/{i}\x07link\x1b]8;;\x07")
                .as_bytes(),
        );
    }
    assert_eq!(uri(parser.screen(), 0, 0), Some("https://example.com/kept"));
    assert_eq!(
        uri(parser.screen(), 1, 0),
        Some("https://example.com/69999")
    );
}
