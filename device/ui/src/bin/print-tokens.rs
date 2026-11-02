//! Print the canonical token sheet so the React page can be generated from it.
//!
//! `scripts/generate-tokens.sh` owns the file header and the destination; this binary owns the
//! sheet itself, because a raw string in the library is the one copy both surfaces read.

fn main() {
    print!("{}", summrise_ui::TOKENS);
}
