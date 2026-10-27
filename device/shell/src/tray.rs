//! Which notification-area entries belong to this executable, and which still need showing.
//!
//! Windows 11 keeps one `NotifyIconSettings` entry per icon id. An earlier process's entry for the
//! same executable stays behind after it exits, so a single promoted entry is not proof that the
//! icon this process just added is shown: every matching entry has to be considered.

/// Whether `path` is this process's executable and the entry is still in the overflow.
///
/// Windows 11 keeps one `NotifyIconSettings` entry per icon id, and an earlier process's entry for
/// the same executable stays behind after it exits. A single promoted entry is therefore not proof
/// that the icon this process just added is shown; every matching entry has to be considered.
pub(crate) fn needs_promotion(own: &str, path: &str, already_promoted: bool) -> bool {
    path.eq_ignore_ascii_case(own) && !already_promoted
}

#[cfg(test)]
mod tests {
    use super::needs_promotion;

    #[test]
    fn a_new_icon_entry_is_promoted_beside_an_older_promoted_one() {
        let own = r"c:\summrise\summrise-shell.exe";
        assert!(needs_promotion(own, r"C:\Summrise\summrise-shell.exe", false));
        assert!(!needs_promotion(own, r"C:\Summrise\summrise-shell.exe", true));
        assert!(!needs_promotion(own, r"C:\Other\thing.exe", false));
    }
}
