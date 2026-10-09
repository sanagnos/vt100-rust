use std::num::NonZeroU16;
use std::sync::Arc;

// text after a longer uri isn't linked, rather than keeping the uri: every
// distinct uri still on the screen or in the scrollback is kept in memory.
const MAX_URI_LEN: usize = 8192;
// vte ignores ids longer than this, too
const MAX_ID_LEN: usize = 250;

/// A hyperlink set with OSC 8 (`\e]8;params;uri\e\\`).
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct Hyperlink {
    uri: String,
    id: Option<String>,
}

impl Hyperlink {
    /// Parses the parameters of an OSC 8 sequence (everything after the
    /// `8`). Returns `Ok(None)` for the sequence which ends a hyperlink (and
    /// for a uri too long or not UTF-8, which ends the previous one too), and
    /// `Err(())` if the sequence is malformed.
    pub(crate) fn parse(params: &[&[u8]]) -> Result<Option<Self>, ()> {
        let [link_params, uri @ ..] = params else {
            return Err(());
        };
        if uri.is_empty() {
            return Err(());
        }
        // uris can contain semicolons, which the osc parser splits on
        let uri = uri.join(&b';');
        if uri.is_empty() || uri.len() > MAX_URI_LEN {
            return Ok(None);
        }
        let Ok(uri) = String::from_utf8(uri) else {
            return Ok(None);
        };
        let id = link_params
            .split(|&b| b == b':')
            .find_map(|param| param.strip_prefix(b"id="))
            .filter(|id| !id.is_empty() && id.len() <= MAX_ID_LEN)
            .map(|id| String::from_utf8_lossy(id).into_owned());
        Ok(Some(Self { uri, id }))
    }

    /// Returns the URI the hyperlink points to.
    #[must_use]
    pub fn uri(&self) -> &str {
        &self.uri
    }

    /// Returns the `id` parameter of the hyperlink, if one was given.
    /// Terminals use it to treat cells which aren't next to each other (for
    /// instance, a link which was split across lines by the application) as
    /// the same link.
    #[must_use]
    pub fn id(&self) -> Option<&str> {
        self.id.as_deref()
    }
}

// how many distinct hyperlinks are kept, and how many bytes of uris and ids.
// a program can print any number of them (codex prints a url again each
// time it grows while streaming), so the least recently used ones are
// dropped, as tmux does (it keeps 5000).
const MAX_LINKS: usize = 4096;
const MAX_BYTES: usize = 1024 * 1024;

#[derive(Clone, Debug)]
struct Entry {
    link: Arc<Hyperlink>,
    // when it was last opened, from Hyperlinks::clock
    used: u64,
}

/// The hyperlinks currently in use, so that cells only need to store a
/// small id rather than the full uri.
#[derive(Clone, Debug, Default)]
pub struct Hyperlinks {
    links: Vec<Option<Entry>>,
    ids: std::collections::HashMap<Arc<Hyperlink>, NonZeroU16>,
    free: Vec<NonZeroU16>,
    bytes: usize,
    clock: u64,
}

fn size(link: &Hyperlink) -> usize {
    link.uri.len() + link.id.as_ref().map_or(0, String::len)
}

impl Hyperlinks {
    pub fn get(&self, id: NonZeroU16) -> Option<&Hyperlink> {
        self.links
            .get(usize::from(id.get() - 1))
            .and_then(Option::as_ref)
            .map(|entry| &*entry.link)
    }

    /// Returns the id of `link` if it is known, marking it as just used.
    pub fn find(&mut self, link: &Hyperlink) -> Option<NonZeroU16> {
        let id = *self.ids.get(link)?;
        self.clock += 1;
        if let Some(entry) = &mut self.links[usize::from(id.get() - 1)] {
            entry.used = self.clock;
        }
        Some(id)
    }

    fn len(&self) -> usize {
        self.ids.len()
    }

    /// Whether `link` can be added without going over the limits.
    pub fn has_room(&self, link: &Hyperlink) -> bool {
        self.len() < MAX_LINKS && self.bytes + size(link) <= MAX_BYTES
    }

    /// Whether a quarter of each limit is free, so that making room isn't
    /// needed again for a while.
    fn roomy(&self, extra: usize) -> bool {
        self.len() <= MAX_LINKS / 4 * 3
            && self.bytes + extra <= MAX_BYTES / 4 * 3
    }

    /// Adds `link`, which must not be known yet; `has_room` must be true.
    pub fn insert(&mut self, link: Hyperlink) -> NonZeroU16 {
        let id = self.free.pop().unwrap_or_else(|| {
            self.links.push(None);
            // there are at most MAX_LINKS ids, and the first is 1
            NonZeroU16::new(u16::try_from(self.links.len()).unwrap()).unwrap()
        });
        self.clock += 1;
        self.bytes += size(&link);
        let link = Arc::new(link);
        self.links[usize::from(id.get() - 1)] = Some(Entry {
            link: Arc::clone(&link),
            used: self.clock,
        });
        self.ids.insert(link, id);
        id
    }

    fn remove(&mut self, id: NonZeroU16) {
        if let Some(entry) = self.links[usize::from(id.get() - 1)].take() {
            self.bytes -= size(&entry.link);
            self.ids.remove(&entry.link);
            self.free.push(id);
        }
    }

    fn ids(&self) -> impl Iterator<Item = NonZeroU16> + '_ {
        self.links.iter().enumerate().filter_map(|(i, slot)| {
            slot.as_ref()?;
            // i + 1 is at most MAX_LINKS
            NonZeroU16::new(u16::try_from(i + 1).unwrap())
        })
    }

    /// Makes room for a link of `extra` bytes: forgets the hyperlinks no
    /// cell has any more (`in_use` is indexed by id), then, if that isn't
    /// enough, the least recently used ones. Returns the ids forgotten while
    /// cells still had them, which those cells must drop.
    pub fn make_room(&mut self, in_use: &[bool], extra: usize) -> Vec<bool> {
        let unused: Vec<_> = self
            .ids()
            .filter(|id| !in_use[usize::from(id.get())])
            .collect();
        for id in unused {
            self.remove(id);
        }
        let mut evicted = vec![false; in_use.len()];
        if !self.roomy(extra) {
            let mut by_age: Vec<_> = self
                .ids()
                .map(|id| {
                    (
                        self.links[usize::from(id.get() - 1)]
                            .as_ref()
                            .unwrap()
                            .used,
                        id,
                    )
                })
                .collect();
            by_age.sort_unstable();
            for (_, id) in by_age {
                if self.roomy(extra) {
                    break;
                }
                self.remove(id);
                evicted[usize::from(id.get())] = true;
            }
        }
        evicted
    }

    /// The size of an `in_use` table for `make_room`.
    pub fn id_limit(&self) -> usize {
        self.links.len() + 1
    }
}

#[cfg(test)]
mod test {
    use super::*;

    fn parse(params: &[&[u8]]) -> Result<Option<Hyperlink>, ()> {
        Hyperlink::parse(params)
    }

    #[test]
    fn parse_params() {
        let link = parse(&[b"id=abc:foo=bar", b"https://example.com/"])
            .unwrap()
            .unwrap();
        assert_eq!(link.uri(), "https://example.com/");
        assert_eq!(link.id(), Some("abc"));

        let link = parse(&[b"", b"https://example.com/?a=1", b"b=2"])
            .unwrap()
            .unwrap();
        assert_eq!(link.uri(), "https://example.com/?a=1;b=2");
        assert_eq!(link.id(), None);

        assert_eq!(parse(&[b"", b""]), Ok(None));
        assert_eq!(parse(&[b"id=abc", b""]), Ok(None));
        assert_eq!(parse(&[b"https://example.com/"]), Err(()));
        assert_eq!(parse(&[]), Err(()));
        let long = vec![b'a'; MAX_URI_LEN + 1];
        assert_eq!(parse(&[b"", &long]), Ok(None));
        assert_eq!(parse(&[b"", b"https://example.com/\xff"]), Ok(None));
    }

    #[test]
    fn find_insert_and_make_room() {
        let link = |uri: &str| Hyperlink {
            uri: uri.to_string(),
            id: None,
        };
        let mut links = Hyperlinks::default();
        let a = links.insert(link("a"));
        let b = links.insert(link("b"));
        assert_ne!(a, b);
        assert_eq!(links.find(&link("a")), Some(a));
        assert_eq!(links.find(&link("c")), None);
        assert_eq!(links.get(b).unwrap().uri(), "b");

        // only b is on the screen: a goes, and nothing has to be evicted
        let mut in_use = vec![false; links.id_limit()];
        in_use[usize::from(b.get())] = true;
        assert!(!links.make_room(&in_use, 0).contains(&true));
        assert_eq!(links.get(a), None);
        assert_eq!(links.get(b).unwrap().uri(), "b");
        assert_eq!(links.insert(link("c")), a);
        assert_eq!(links.find(&link("b")), Some(b));
    }

    #[test]
    fn evicts_the_least_recently_used() {
        let mut links = Hyperlinks::default();
        let ids: Vec<_> = (0..MAX_LINKS)
            .map(|i| {
                links.insert(Hyperlink {
                    uri: format!("https://example.com/{i}"),
                    id: None,
                })
            })
            .collect();
        let first = Hyperlink {
            uri: "https://example.com/0".to_string(),
            id: None,
        };
        // using the oldest makes it the newest
        assert_eq!(links.find(&first), Some(ids[0]));
        assert!(!links.has_room(&first));
        let in_use = vec![true; links.id_limit()];
        let evicted = links.make_room(&in_use, 10);
        assert!(links.roomy(10));
        assert_eq!(evicted.iter().filter(|&&e| e).count(), MAX_LINKS / 4);
        assert!(!evicted[usize::from(ids[0].get())]);
        assert!(evicted[usize::from(ids[1].get())]);
        assert!(!evicted[usize::from(ids[MAX_LINKS - 1].get())]);
        assert_eq!(links.get(ids[1]), None);
        assert_eq!(links.get(ids[0]).unwrap().uri(), "https://example.com/0");
    }

    #[test]
    fn stays_bounded_while_streaming() {
        // as codex streams urls: each drawn again as it grows, and every
        // version kept in the scrollback
        let mut parser = crate::Parser::new(
            crate::Parser::<()>::DEFAULT_ROWS,
            crate::Parser::<()>::DEFAULT_COLS,
            10_000,
        );
        let mut url = String::new();
        for i in 0..10_000 {
            url = format!(
                "https://example.com/{}/{}",
                i / 100,
                "x".repeat(i % 100)
            );
            parser.process(
                format!("\x1b]8;;{url}\x07link {i}\x1b]8;;\x07\r\n")
                    .as_bytes(),
            );
            let table = parser.screen().hyperlink_table();
            assert!(table.len() <= MAX_LINKS);
            assert!(table.bytes <= MAX_BYTES);
        }
        // the newest links still work; the oldest were dropped from their
        // cells rather than pointing at other links
        assert_eq!(parser.screen().cell_hyperlink(22, 0).unwrap().uri(), url);
        parser.screen_mut().set_scrollback(10_000);
        let top = parser.screen().contents();
        assert!(top.starts_with("link 0\n"), "{top:.20}");
        assert_eq!(parser.screen().cell_hyperlink(0, 0), None);
    }

    #[test]
    fn byte_budget() {
        let mut links = Hyperlinks::default();
        let long = |i: usize| Hyperlink {
            uri: format!("https://example.com/{i}/{}", "a".repeat(8000)),
            id: None,
        };
        let mut i = 0;
        while links.has_room(&long(i)) {
            links.insert(long(i));
            i += 1;
        }
        assert!(i < 200);
        assert!(links.bytes <= MAX_BYTES);
    }
}
