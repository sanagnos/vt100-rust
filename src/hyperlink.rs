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

/// The hyperlinks currently in use, so that cells only need to store a
/// small id rather than the full uri.
#[derive(Clone, Debug, Default)]
pub struct Hyperlinks {
    links: Vec<Option<Arc<Hyperlink>>>,
    ids: std::collections::HashMap<Arc<Hyperlink>, NonZeroU16>,
    free: Vec<NonZeroU16>,
}

impl Hyperlinks {
    pub fn get(&self, id: NonZeroU16) -> Option<&Hyperlink> {
        self.links
            .get(usize::from(id.get() - 1))
            .and_then(Option::as_deref)
    }

    /// Returns the id for `link`, adding it if it isn't known yet. Returns
    /// `None` if the table is full; `sweep` can make room.
    pub fn intern(&mut self, link: Hyperlink) -> Option<NonZeroU16> {
        if let Some(&id) = self.ids.get(&link) {
            return Some(id);
        }
        let id = if let Some(id) = self.free.pop() {
            id
        } else {
            let id = u16::try_from(self.links.len() + 1).ok()?;
            self.links.push(None);
            // id is at least 1, since it is one past an index
            NonZeroU16::new(id).unwrap()
        };
        let link = Arc::new(link);
        self.links[usize::from(id.get() - 1)] = Some(Arc::clone(&link));
        self.ids.insert(link, id);
        Some(id)
    }

    pub fn is_full(&self) -> bool {
        self.free.is_empty() && self.links.len() >= usize::from(u16::MAX)
    }

    /// Forgets every hyperlink whose id `in_use` doesn't return true for.
    pub fn sweep(&mut self, in_use: impl Fn(NonZeroU16) -> bool) {
        for (i, slot) in self.links.iter_mut().enumerate() {
            // i is less than u16::MAX, since that is the most ids we hand out
            let id = NonZeroU16::new(u16::try_from(i + 1).unwrap()).unwrap();
            if slot.is_some() && !in_use(id) {
                if let Some(link) = slot.take() {
                    self.ids.remove(&link);
                }
                self.free.push(id);
            }
        }
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
    fn intern_and_sweep() {
        let link = |uri: &str| Hyperlink {
            uri: uri.to_string(),
            id: None,
        };
        let mut links = Hyperlinks::default();
        let a = links.intern(link("a")).unwrap();
        let b = links.intern(link("b")).unwrap();
        assert_ne!(a, b);
        assert_eq!(links.intern(link("a")), Some(a));
        assert_eq!(links.get(b).unwrap().uri(), "b");

        links.sweep(|id| id == b);
        assert_eq!(links.get(a), None);
        assert_eq!(links.get(b).unwrap().uri(), "b");
        let c = links.intern(link("c")).unwrap();
        assert_eq!(c, a);
        assert_eq!(links.intern(link("b")), Some(b));
    }
}
