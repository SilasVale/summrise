//! THE TWO LANDING ARMS — the documents `index/landing` generates, read from the committed modules.
//!
//! ## Why the arms are MODULES and not `.html` files
//!
//! `index/landing/build.sh` writes `index/src/landing/setup.js` and `npm-only.js` — JavaScript modules
//! whose default export is the document as a template literal — and the reason is in the generator's
//! own comment: the document has two consumers that do not share a module system. The worker imports
//! it as text; plain Node (the index tests, `landing-check.mjs`, the landing design sweep) cannot
//! import a `.html` at all (`ERR_UNKNOWN_FILE_EXTENSION`). A module satisfies both, and the document
//! inside it is still a document a person can read.
//!
//! So the extraction below is the one place the module WRAPPER is removed, and it is deliberately
//! small: the first backtick to the last, with the module's own `// GENERATED …` header and its
//! `export default` outside. **A test pins that the extraction yields a document that still contains
//! its slots**, because an extraction that quietly returns the wrapper would compile, run, and serve a
//! page with `{{INSTALLER_URL}}` in the href.

/// The generated module, embedded at compile time — the same file the JavaScript worker imports.
const SETUP_JS: &str = include_str!("../../src/landing/setup.js");
const NPM_ONLY_JS: &str = include_str!("../../src/landing/npm-only.js");

/// The arm for a release that PUBLISHED a Windows installer.
pub fn setup_arm() -> &'static str {
    extract(SETUP_JS)
}

/// The arm for a release that did not — the tgz-only path, which has no door at all.
pub fn npm_only_arm() -> &'static str {
    extract(NPM_ONLY_JS)
}

// `'static` IS A PROPERTY OF THE CONSTANTS, NOT OF A PARAMETER. The first version took `&str` and
// returned `&'static str`; the borrow checker refused, and it was RIGHT to — a function promising
// `'static` from an arbitrary `&str` could only be lying.

/// The document inside a generated module: the text between the first backtick and the last.
///
/// A generated module is a template literal, and the document may itself contain backticks (the
/// inline `<code>` copy command does), so the boundaries are the FIRST and the LAST and not a
/// non-greedy match.
fn extract(module: &'static str) -> &'static str {
    let open = module
        .find('`')
        .expect("the generated module has no opening backtick")
        + 1;
    let close = module
        .rfind('`')
        .expect("the generated module has no closing backtick");
    &module[open..close]
}

/// The landing's ETag: a SHA-256 of the rendered body, the first 32 hex characters, quoted.
///
/// **IT IS OF THE RENDERED BODY, NOT OF A FILE.** Two requests that render the same document with the
/// same three URLs get the same tag, and a request whose slots differ gets a different one — which is
/// the property a device's `if-none-match` revalidation depends on.
pub fn etag(body: &str) -> String {
    let digest = sha256(body.as_bytes());
    let hex: String = digest.iter().map(|b| format!("{b:02x}")).collect();
    format!("\"{}\"", &hex[..32])
}

/// SHA-256, and **THE `sha2` CRATE IS NOT A DEPENDENCY FOR THIS**: a page is 30 KB and hashed once
/// per request on the edge, which is not a hot path — but the crate IS about to hash installer-sized
/// bodies, and a hand-rolled SHA-256 is exactly the kind of "it is only 64 lines" that the
/// `regex` line in `url.rs` and the `str::find` line in `attention.rs` both came from. So it is
/// `sha2`, and the measurement to watch is the wasm size, not the line count.
fn sha256(bytes: &[u8]) -> [u8; 32] {
    use sha2::{Digest, Sha256};
    let mut h = Sha256::new();
    h.update(bytes);
    h.finalize().into()
}

#[cfg(test)]
// SAME ALLOW AND SAME REASON as the other two modules: the names assert the behaviour a reader
// expects NOT to find, and the capitals are the note.
#[allow(non_snake_case)]
mod tests {
    use super::*;
    use crate::SETUP_SLOT;

    #[test]
    fn the_extraction_yields_a_DOCUMENT_and_not_the_module_wrapper() {
        // THE CHECK THAT MATTERS: an extraction that returned the wrapper would compile, run, and
        // serve a page whose href is the literal text `{{INSTALLER_URL}}`. So the extracted text
        // must contain the SLOTS and must not contain the module's own syntax.
        for arm in [setup_arm(), npm_only_arm()] {
            assert!(
                arm.contains("{{INSTALLER_URL}}"),
                "the slots must survive the extraction"
            );
            assert!(
                !arm.contains("export default"),
                "the module wrapper must be gone"
            );
            assert!(
                arm.trim_start().starts_with("<!doctype html"),
                "{}",
                &arm[..40.min(arm.len())]
            );
        }
    }

    #[test]
    fn the_two_arms_are_DIFFERENT_documents_and_only_one_carries_the_SETUP_slot() {
        let setup = setup_arm();
        let npm_only = npm_only_arm();
        assert_ne!(
            setup, npm_only,
            "the two arms must not be the same document"
        );
        // The door is the SLOT, not a literal filename: the generator leaves `{{SETUP_URL}}` and the
        // per-request fill decides the URL. **The first version of this test asserted
        // `SummriseAgent-Setup.exe` in the setup arm and it is not there** — the only place that
        // string appears is the WHITELIST FALLBACK, not the document. The assertion that says what
        // the arms actually are is the slot one, and the one that says the tgz-only path is honest
        // is the sentence it carries INSTEAD.
        assert!(
            setup.contains(SETUP_SLOT),
            "the setup arm carries the door's slot"
        );
        assert!(
            !npm_only.contains(SETUP_SLOT),
            "the npm-only arm has no door at all"
        );
        // round 125 in one line: the release that published no installer SAYS SO on the page.
        assert!(
            npm_only.contains("No Windows installer is published for this release"),
            "the tgz-only arm states the absence rather than offering the previous build",
        );
    }

    #[test]
    fn the_etag_is_32_hex_characters_and_quoted() {
        let e = etag("hello");
        assert_eq!(e.len(), 34, "32 hex + two quotes: {e}");
        assert!(e.starts_with('"') && e.ends_with('"'), "{e}");
        assert!(e[1..33].chars().all(|c| c.is_ascii_hexdigit()), "{e}");
        // The same body gets the same tag, and a different body does not.
        assert_eq!(etag("hello"), e);
        assert_ne!(etag("hello "), e);
    }
}
