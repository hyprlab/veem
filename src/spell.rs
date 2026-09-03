//! Subject-line spell checking (#114): the same enchant engine WebKit uses
//! for the message body, reached directly — GTK entries have no checker of
//! their own, and libspelling isn't in the runtime.
//!
//! enchant is dlopen'd at runtime rather than linked: it is guaranteed
//! present wherever WebKitGTK is (the GNOME runtime included), but linking
//! at build time would demand its -devel package on every build machine.
//! If the library or the dictionary is missing, checking simply reports
//! nothing misspelled — same silence the body would show.

use std::cell::RefCell;
use std::ffi::{c_char, c_int, c_void, CString};
use std::rc::Rc;

extern "C" {
    // From glibc itself (libdl merged into libc since 2.34) — no crate, no
    // link flag needed beyond what every binary already has.
    fn dlopen(filename: *const c_char, flag: c_int) -> *mut c_void;
    fn dlsym(handle: *mut c_void, symbol: *const c_char) -> *mut c_void;
}

type BrokerInit = unsafe extern "C" fn() -> *mut c_void;
type RequestDict = unsafe extern "C" fn(*mut c_void, *const c_char) -> *mut c_void;
type DictCheck = unsafe extern "C" fn(*mut c_void, *const c_char, isize) -> c_int;

pub struct SpellChecker {
    dict: *mut c_void,
    check: DictCheck,
}

impl SpellChecker {
    /// A checker for `lang`, or `None` when enchant or the dictionary is
    /// unavailable. The broker and dictionary are deliberately never freed:
    /// one per language per process, alive for its whole life.
    fn new(lang: &str) -> Option<SpellChecker> {
        unsafe {
            let so = CString::new("libenchant-2.so.2").ok()?;
            let lib = dlopen(so.as_ptr(), 1 /* RTLD_LAZY */);
            if lib.is_null() {
                return None;
            }
            let sym = |name: &str| {
                let n = CString::new(name).unwrap();
                dlsym(lib, n.as_ptr())
            };
            let init = sym("enchant_broker_init");
            let req = sym("enchant_broker_request_dict");
            let check = sym("enchant_dict_check");
            if init.is_null() || req.is_null() || check.is_null() {
                return None;
            }
            let init: BrokerInit = std::mem::transmute(init);
            let req: RequestDict = std::mem::transmute(req);
            let check: DictCheck = std::mem::transmute(check);
            let broker = init();
            if broker.is_null() {
                return None;
            }
            let tag = CString::new(lang).ok()?;
            let dict = req(broker, tag.as_ptr());
            if dict.is_null() {
                return None;
            }
            Some(SpellChecker { dict, check })
        }
    }

    /// Whether enchant knows no such word. Errors (and interior NULs) count
    /// as correctly spelled — a checker must never cry wolf.
    fn is_misspelled(&self, word: &str) -> bool {
        let Ok(w) = CString::new(word) else { return false };
        unsafe { (self.check)(self.dict, w.as_ptr(), word.len() as isize) > 0 }
    }
}

thread_local! {
    /// The current checker, keyed by the language it was built for so a
    /// settings change swaps it.
    static CHECKER: RefCell<Option<(String, Option<Rc<SpellChecker>>)>> =
        const { RefCell::new(None) };
}

fn checker_for(lang: &str) -> Option<Rc<SpellChecker>> {
    CHECKER.with(|c| {
        let mut slot = c.borrow_mut();
        match slot.as_ref() {
            Some((l, found)) if l == lang => found.clone(),
            _ => {
                let built = SpellChecker::new(lang).map(Rc::new);
                *slot = Some((lang.to_string(), built.clone()));
                built
            }
        }
    })
}

/// Subject-line prefixes and mail shorthand no dictionary carries.
const MAIL_WORDS: &[&str] = &["Re", "RE", "re", "Fwd", "FWD", "Fw", "FW"];

/// The words in `text` worth checking, as (byte start, byte end, word).
/// Whitespace-separated tokens carrying an '@', a digit, or a scheme are
/// skipped whole — addresses, versions and links aren't prose — and within
/// the rest, maximal alphabetic runs (apostrophes included) of two letters
/// or more are the words.
fn checkable_words(text: &str) -> Vec<(usize, usize, &str)> {
    let mut out = Vec::new();
    let mut token_start = 0;
    for token in text.split_whitespace() {
        let start = token_start + text[token_start..].find(token).unwrap_or(0);
        token_start = start + token.len();
        if token.contains('@') || token.contains("://") || token.chars().any(|c| c.is_ascii_digit())
        {
            continue;
        }
        let mut word_start: Option<usize> = None;
        // One past the end so a trailing word closes like an interior one.
        for (i, ch) in token.char_indices().chain(std::iter::once((token.len(), ' '))) {
            let wordish = ch.is_alphabetic() || ch == '\'' || ch == '\u{2019}';
            match (word_start, wordish) {
                (None, true) => word_start = Some(i),
                (Some(ws), false) => {
                    let w = &token[ws..i];
                    if w.chars().filter(|c| c.is_alphabetic()).count() >= 2
                        && !MAIL_WORDS.contains(&w)
                    {
                        out.push((start + ws, start + i, w));
                    }
                    word_start = None;
                }
                _ => {}
            }
        }
    }
    out
}

/// Pango error-underline attributes for every misspelled word in `text` —
/// `None` (clear the entry's attributes) when checking is off or enchant is
/// unavailable.
pub fn error_attrs(text: &str) -> Option<gtk::pango::AttrList> {
    if !crate::config::load_spellcheck() {
        return None;
    }
    let lang = crate::ui::rich_editor::resolved_spell_language();
    let checker = checker_for(&lang)?;
    let attrs = gtk::pango::AttrList::new();
    for (start, end, word) in checkable_words(text) {
        if checker.is_misspelled(word) {
            let mut a = gtk::pango::AttrInt::new_underline(gtk::pango::Underline::Error);
            a.set_start_index(start as u32);
            a.set_end_index(end as u32);
            attrs.insert(a);
        }
    }
    Some(attrs)
}

#[cfg(test)]
mod tests {
    use super::checkable_words;

    #[test]
    fn words_are_found_and_noise_is_skipped() {
        let words: Vec<&str> =
            checkable_words("Re: meet ada@x.com at 3pm — v1.2 recieve https://x.y ok")
                .iter()
                .map(|(_, _, w)| *w)
                .collect();
        // "Re" is mail shorthand, the address/version/time/link tokens carry
        // digits or schemes, and single letters aren't words. "at" IS a word
        // — short, but the dictionary knows it, so it never underlines.
        assert_eq!(words, ["meet", "at", "recieve", "ok"]);
    }

    #[test]
    fn byte_ranges_point_at_the_words() {
        let text = "héllo wörld";
        for (s, e, w) in checkable_words(text) {
            assert_eq!(&text[s..e], w);
        }
        assert_eq!(checkable_words(text).len(), 2);
    }
}
