//! The append-only memory capability (design §4.5).
//!
//! One JSON object per line under `<data>/memory.jsonl`. Appends never rewrite earlier entries; a
//! search reads the file and walks it backwards, so the newest entry is returned first. The caps are
//! part of the capability, not a client convention: **1 MiB per entry's serialized JSON** (tags and
//! JSON escaping included, the trailing newline not counted), 32 tags, 128 bytes per tag, 10 000
//! entries, 100 results.
//!
//! The entry cap is a storage cap, not a text-length cap: the check happens after `serde_json`
//! writes the object, so a quote-heavy 1 MiB text cannot serialize to two megabytes on disk.
//!
//! The MCP surface serves several sessions at once, so one lock makes the file's invariants hold
//! under concurrency: an append's count-then-write is one critical section (two racing appends
//! cannot both take the last slot), and a search takes the read side, so it never reads a line that
//! is halfway written.

use std::collections::VecDeque;
use std::fmt;
use std::fs::{self, OpenOptions};
use std::io::{self, BufRead, BufReader, Write};
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

/// The largest serialized entry: tags, JSON quoting and escapes included; the newline is extra.
pub const MAX_ENTRY_BYTES: usize = 1024 * 1024;

/// The most tags one entry may carry.
pub const MAX_TAGS: usize = 32;

/// The largest one tag may be, in bytes.
pub const MAX_TAG_BYTES: usize = 128;

/// The largest number of entries the file may hold.
pub const MAX_ENTRIES: usize = 10_000;

/// The largest number of results one search may return.
pub const MAX_RESULTS: usize = 100;

/// The most a `memory.export` returns in one call: a tool result is a JSON-RPC frame, and a store at
/// its entry ceiling can be gigabytes.
pub const MAX_EXPORT_BYTES: usize = 2 * 1024 * 1024;

/// What `export` returns: the stored lines, how many whole lines they are, and whether the cap cut
/// the store.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Export {
    /// The store's own bytes, as JSONL, ending on a newline unless the store is empty.
    pub text: String,
    /// Whole lines in [`Export::text`].
    pub entries: usize,
    /// The store was larger than the cap and `text` is a prefix.
    pub truncated: bool,
    /// The store's size, so a caller can see what it did not get.
    pub total_bytes: usize,
}

/// One remembered fact.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Entry {
    /// The entry's text.
    pub text: String,
    /// Free-form tags; a search matches them as well as the text.
    pub tags: Vec<String>,
}

/// Why an append or a search was refused or failed.
#[derive(Debug)]
pub enum MemoryError {
    /// The serialized entry exceeds [`MAX_ENTRY_BYTES`].
    TooLarge {
        /// The payload's size in bytes; after serialization this is the stored line's length.
        bytes: usize,
        /// The cap.
        max: usize,
    },
    /// The entry carries more than [`MAX_TAGS`] tags.
    TooManyTags {
        /// The number of tags given.
        count: usize,
        /// The cap.
        max: usize,
    },
    /// One tag is longer than [`MAX_TAG_BYTES`].
    TagTooLong {
        /// The tag's length in bytes.
        len: usize,
        /// The cap.
        max: usize,
    },
    /// The file already holds [`MAX_ENTRIES`] entries; append-only means no eviction.
    Full {
        /// The cap.
        max: usize,
    },
    /// The requested limit exceeds [`MAX_RESULTS`].
    LimitTooLarge {
        /// The requested limit.
        limit: usize,
        /// The cap.
        max: usize,
    },
    /// A line is not the JSON object this capability writes; the line number is 1-based.
    Corrupt {
        /// The offending line.
        line: usize,
    },
    /// The filesystem refused the call.
    Io(io::Error),
}

impl fmt::Display for MemoryError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            MemoryError::TooLarge { bytes, max } => {
                write!(f, "the entry is {bytes} bytes; the cap is {max}")
            }
            MemoryError::TooManyTags { count, max } => {
                write!(f, "the entry has {count} tags; the cap is {max}")
            }
            MemoryError::TagTooLong { len, max } => {
                write!(f, "a tag is {len} bytes; the cap is {max}")
            }
            MemoryError::Full { max } => {
                write!(f, "memory holds its maximum of {max} entries")
            }
            MemoryError::LimitTooLarge { limit, max } => {
                write!(f, "a search returns at most {max} entries, not {limit}")
            }
            MemoryError::Corrupt { line } => write!(f, "memory line {line} is not valid JSON"),
            MemoryError::Io(error) => write!(f, "{error}"),
        }
    }
}

impl std::error::Error for MemoryError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            MemoryError::Io(error) => Some(error),
            _ => None,
        }
    }
}

/// The device's memory, rooted at one data directory.
pub struct Memory {
    path: PathBuf,
    /// One writer at a time, and readers that never overlap a write.
    lock: std::sync::RwLock<()>,
}

impl Memory {
    /// The memory under `data`: the file is `<data>/memory.jsonl`.
    pub fn new(data: &Path) -> Memory {
        Memory {
            path: data.join("memory.jsonl"),
            lock: std::sync::RwLock::new(()),
        }
    }

    /// Append one entry.
    ///
    /// Validation happens before the write lock and before the serialized line is built where it
    /// can: the tag count and each tag's length first, then the JSON serialization, and finally the
    /// stored line's own length. The cap is on what is written, not on `text.len()`, because tags
    /// and escaping are part of the line.
    pub fn append(&self, text: &str, tags: &[String]) -> Result<(), MemoryError> {
        if tags.len() > MAX_TAGS {
            return Err(MemoryError::TooManyTags {
                count: tags.len(),
                max: MAX_TAGS,
            });
        }
        if let Some(tag) = tags.iter().find(|tag| tag.len() > MAX_TAG_BYTES) {
            return Err(MemoryError::TagTooLong {
                len: tag.len(),
                max: MAX_TAG_BYTES,
            });
        }

        // A cheap pre-check refuses an obviously oversized payload before serde allocates its
        // escaped form. The authoritative check is the serialized length below.
        let raw_bytes = text.len() + tags.iter().map(String::len).sum::<usize>();
        if raw_bytes > MAX_ENTRY_BYTES {
            return Err(MemoryError::TooLarge {
                bytes: raw_bytes,
                max: MAX_ENTRY_BYTES,
            });
        }

        let entry = Entry {
            text: text.to_owned(),
            tags: tags.to_vec(),
        };
        let mut line = serde_json::to_string(&entry)
            .map_err(|error| MemoryError::Io(io::Error::new(io::ErrorKind::InvalidData, error)))?;
        if line.len() > MAX_ENTRY_BYTES {
            // A text of quotes or control characters is larger on disk than in memory. The cap is
            // the stored line, so it is checked after serialization; nothing has been written yet.
            return Err(MemoryError::TooLarge {
                bytes: line.len(),
                max: MAX_ENTRY_BYTES,
            });
        }
        line.push('\n');

        // The count and the write are one critical section: the last slot cannot be taken twice.
        let _writing = self
            .lock
            .write()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        if let Some(parent) = self.path.parent() {
            fs::create_dir_all(parent).map_err(MemoryError::Io)?;
        }
        if self.count_entries()? >= MAX_ENTRIES {
            // Append-only means the cap is a refusal, not an eviction of the oldest entry.
            return Err(MemoryError::Full { max: MAX_ENTRIES });
        }

        // `append(true)` is the capability's one invariant: an existing byte is never rewritten.
        let mut file = OpenOptions::new()
            .append(true)
            .create(true)
            .open(&self.path)
            .map_err(MemoryError::Io)?;
        file.write_all(line.as_bytes()).map_err(MemoryError::Io)?;
        file.flush().map_err(MemoryError::Io)
    }

    /// Search entries newest-first, matching `query` case-insensitively over text and tags.
    pub fn search(&self, query: &str, limit: usize) -> Result<Vec<Entry>, MemoryError> {
        if limit > MAX_RESULTS {
            return Err(MemoryError::LimitTooLarge {
                limit,
                max: MAX_RESULTS,
            });
        }
        if limit == 0 {
            return Ok(Vec::new());
        }

        // A search never reads a line an append is still writing.
        let _reading = self
            .lock
            .read()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let file = match fs::File::open(&self.path) {
            Ok(file) => file,
            Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(Vec::new()),
            Err(error) => return Err(MemoryError::Io(error)),
        };

        let needle = query.to_lowercase();
        let mut found: VecDeque<Entry> = VecDeque::new();
        for (index, line) in BufReader::new(file).lines().enumerate() {
            let line = line.map_err(MemoryError::Io)?;
            if line.trim().is_empty() {
                continue;
            }
            let entry: Entry = serde_json::from_str(&line)
                .map_err(|_| MemoryError::Corrupt { line: index + 1 })?;
            if entry.text.to_lowercase().contains(&needle)
                || entry
                    .tags
                    .iter()
                    .any(|tag| tag.to_lowercase().contains(&needle))
            {
                // Walking forwards but pushing to the front leaves the newest entries first, and
                // popping the back keeps at most `limit` entries in memory.
                found.push_front(entry);
                if found.len() > limit {
                    found.pop_back();
                }
            }
        }
        Ok(found.into())
    }

    /// Enumerate entries, newest first, optionally only those carrying `tag`.
    ///
    /// `search` answers "what do I know about X"; this answers "what do I know". The tag match is
    /// exact and case-insensitive — a substring match would make `ops` list everything tagged
    /// `ops-runbook` — and the limit is the same cap as a search's, because both are a client
    /// reading a list it must render.
    pub fn list(&self, limit: usize, tag: Option<&str>) -> Result<Vec<Entry>, MemoryError> {
        if limit > MAX_RESULTS {
            return Err(MemoryError::LimitTooLarge {
                limit,
                max: MAX_RESULTS,
            });
        }
        if limit == 0 {
            return Ok(Vec::new());
        }

        let _reading = self
            .lock
            .read()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let file = match fs::File::open(&self.path) {
            Ok(file) => file,
            Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(Vec::new()),
            Err(error) => return Err(MemoryError::Io(error)),
        };

        let wanted = tag.map(str::to_lowercase);
        let mut found: VecDeque<Entry> = VecDeque::new();
        for (index, line) in BufReader::new(file).lines().enumerate() {
            let line = line.map_err(MemoryError::Io)?;
            if line.trim().is_empty() {
                continue;
            }
            let entry: Entry = serde_json::from_str(&line)
                .map_err(|_| MemoryError::Corrupt { line: index + 1 })?;
            if let Some(wanted) = &wanted {
                if !entry.tags.iter().any(|tag| tag.to_lowercase() == *wanted) {
                    continue;
                }
            }
            found.push_front(entry);
            if found.len() > limit {
                found.pop_back();
            }
        }
        Ok(found.into())
    }

    /// The store as JSONL text, cut at a line boundary when it is larger than `max_bytes`.
    ///
    /// An export is for a backup, so it is the file's own bytes rather than a re-serialization that
    /// might not round-trip. The cap exists because a tool result is a JSON-RPC frame, not a stream:
    /// a store at its 10 000-entry ceiling can be gigabytes, and a client that asked for everything
    /// can ask again with the byte offset the truncation names.
    pub fn export(&self, max_bytes: usize) -> Result<Export, MemoryError> {
        let _reading = self
            .lock
            .read()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let bytes = match fs::read(&self.path) {
            Ok(bytes) => bytes,
            Err(error) if error.kind() == io::ErrorKind::NotFound => Vec::new(),
            Err(error) => return Err(MemoryError::Io(error)),
        };
        let total = bytes.len();
        let truncated = total > max_bytes;
        let kept = if truncated {
            // The last newline at or before the cap; a store with no newline in the window exports
            // an empty text rather than half a JSON object.
            match bytes[..max_bytes].iter().rposition(|byte| *byte == b'\n') {
                Some(last) => &bytes[..=last],
                None => &bytes[..0],
            }
        } else {
            &bytes[..]
        };
        let entries = kept.iter().filter(|byte| **byte == b'\n').count();
        Ok(Export {
            text: String::from_utf8_lossy(kept).into_owned(),
            entries,
            truncated,
            total_bytes: total,
        })
    }

    /// How many non-empty lines the file holds, streamed so a large file is not loaded whole.
    fn count_entries(&self) -> Result<usize, MemoryError> {
        let file = match fs::File::open(&self.path) {
            Ok(file) => file,
            Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(0),
            Err(error) => return Err(MemoryError::Io(error)),
        };
        let mut count = 0;
        for line in BufReader::new(file).lines() {
            let line = line.map_err(MemoryError::Io)?;
            if !line.trim().is_empty() {
                count += 1;
            }
        }
        Ok(count)
    }
}

#[cfg(test)]
mod tests {
    use std::fs;

    use super::*;
    use crate::test_support::TempDir;

    fn memory(label: &str) -> (TempDir, Memory) {
        let dir = TempDir::new(label);
        let memory = Memory::new(&dir.join("data"));
        (dir, memory)
    }

    fn json_line(text: &str, tags: &[&str]) -> String {
        let tags = tags
            .iter()
            .map(|tag| format!("\"{tag}\""))
            .collect::<Vec<_>>()
            .join(",");
        format!("{{\"text\":\"{text}\",\"tags\":[{tags}]}}")
    }

    fn write_fixture(memory: &Memory, entries: usize) {
        let line = json_line("fixture", &[]);
        let whole = std::iter::repeat_n(line, entries)
            .collect::<Vec<_>>()
            .join("\n");
        let parent = memory.path.parent().expect("the data directory");
        fs::create_dir_all(parent).expect("the data directory is creatable");
        fs::write(&memory.path, format!("{whole}\n")).expect("the fixture is written");
    }

    #[test]
    fn append_then_search_round_trips_newest_first_matching_text_and_tags() {
        let (dir, memory) = memory("memory-round-trip");
        assert_eq!(
            memory.path,
            dir.join("data").join("memory.jsonl"),
            "the capability owns exactly one file under the data directory"
        );

        memory
            .append("the first note", &["alpha".to_owned()])
            .expect("the first append");
        memory
            .append("the second note", &["beta".to_owned()])
            .expect("the second append");
        memory
            .append("a third note", &["alpha".to_owned(), "gamma".to_owned()])
            .expect("the third append");

        let all = memory
            .search("", 10)
            .expect("everything matches the empty query");
        let texts: Vec<&str> = all.iter().map(|entry| entry.text.as_str()).collect();
        assert_eq!(
            texts,
            ["a third note", "the second note", "the first note"],
            "newest first is the file read backwards"
        );
        assert_eq!(all[0].tags, ["alpha", "gamma"]);

        assert_eq!(
            memory
                .search("SECOND", 10)
                .expect("case-insensitive search")
                .len(),
            1
        );
        assert_eq!(
            memory
                .search("gamma", 10)
                .expect("a tag matches like text")
                .len(),
            1
        );
        assert_eq!(
            memory
                .search("third note", 10)
                .expect("a substring matches")
                .len(),
            1
        );
        assert!(memory
            .search("absent", 10)
            .expect("no match is not an error")
            .is_empty());
    }

    #[test]
    fn search_honours_the_limit_and_refuses_more_than_one_hundred() {
        let (_dir, memory) = memory("memory-limit");
        for index in 0..3 {
            memory
                .append(&format!("note {index}"), &[])
                .expect("a small append");
        }

        let two = memory.search("", 2).expect("two is a legal limit");
        let texts: Vec<&str> = two.iter().map(|entry| entry.text.as_str()).collect();
        assert_eq!(texts, ["note 2", "note 1"]);

        let error = memory
            .search("", MAX_RESULTS + 1)
            .expect_err("a limit over the cap is refused, not silently clamped");
        assert!(
            matches!(
                error,
                MemoryError::LimitTooLarge {
                    limit: 101,
                    max: 100
                }
            ),
            "{error:?}"
        );
    }

    #[test]
    fn an_entry_larger_than_one_mebibyte_is_refused_and_writes_nothing() {
        let (_dir, memory) = memory("memory-entry-cap");
        let text = "x".repeat(MAX_ENTRY_BYTES + 1);

        let error = memory
            .append(&text, &[])
            .expect_err("an entry over the cap is refused");

        assert!(matches!(error, MemoryError::TooLarge { .. }), "{error:?}");
        assert!(
            memory
                .search("", 10)
                .expect("the memory is readable")
                .is_empty(),
            "a refused append writes no line"
        );

        let at_the_edge = "x".repeat(MAX_ENTRY_BYTES);
        let error = memory
            .append(&at_the_edge, &["y".to_owned()])
            .expect_err("tags count toward the entry's bytes");
        assert!(matches!(error, MemoryError::TooLarge { .. }), "{error:?}");
    }

    #[test]
    fn a_tag_flood_is_refused_by_count_before_it_is_stored() {
        let (_dir, memory) = memory("memory-tag-flood");

        // The review's probe: 500 000 empty tags, which the old text+tag sum counted as zero bytes.
        let tags = vec![String::new(); 500_000];
        let error = memory
            .append("", &tags)
            .expect_err("a tag flood is not an entry of zero bytes");
        assert!(
            matches!(
                error,
                MemoryError::TooManyTags {
                    count: 500_000,
                    max: 32
                }
            ),
            "{error:?}"
        );
        assert!(
            !memory.path.exists(),
            "a refused append must not create the memory file"
        );

        let tags = vec!["tag".to_owned(); MAX_TAGS + 1];
        let error = memory
            .append("some text", &tags)
            .expect_err("one tag over the cap is refused");
        assert!(
            matches!(error, MemoryError::TooManyTags { count: 33, max: 32 }),
            "{error:?}"
        );
    }

    #[test]
    fn a_tag_longer_than_the_cap_is_refused() {
        let (_dir, memory) = memory("memory-tag-length");

        let tags = vec!["x".repeat(MAX_TAG_BYTES + 1)];
        let error = memory
            .append("the text is irrelevant", &tags)
            .expect_err("a tag over its own cap is refused");
        assert!(
            matches!(error, MemoryError::TagTooLong { len: 129, max: 128 }),
            "{error:?}"
        );

        // Exactly at both caps is legal: the tag count and each tag's length are bounds, not ranges.
        let tags = vec!["y".repeat(MAX_TAG_BYTES); MAX_TAGS];
        memory
            .append("", &tags)
            .expect("32 tags of 128 bytes each are inside both caps");
    }

    #[test]
    fn the_cap_is_the_serialized_line_not_the_raw_text() {
        let (_dir, memory) = memory("memory-escape-cap");

        // One MiB of quotes is one MiB in memory but roughly two on disk.
        let text = "\"".repeat(MAX_ENTRY_BYTES);
        let error = memory
            .append(&text, &[])
            .expect_err("the serialized line is what the cap bounds");
        assert!(
            matches!(error, MemoryError::TooLarge { bytes, max } if bytes > max),
            "the refusal reports the serialized size: {error:?}"
        );
        assert!(
            !memory.path.exists(),
            "a refused append writes no line and no file"
        );

        // A quote-light text of the same length fits, because its serialized form is small.
        let text = "x".repeat(MAX_ENTRY_BYTES - 64);
        memory
            .append(&text, &[])
            .expect("a text whose serialized line fits the cap is accepted");
        let stored = fs::read_to_string(&memory.path).expect("the memory is readable");
        assert_eq!(stored.lines().count(), 1);
        assert!(
            stored.len() <= MAX_ENTRY_BYTES + 1,
            "the stored line plus its newline is inside the cap: {} bytes",
            stored.len()
        );
    }

    #[test]
    fn concurrent_appends_never_exceed_the_entry_cap() {
        let (_dir, memory) = memory("memory-concurrent-cap");
        write_fixture(&memory, MAX_ENTRIES - 1);
        let barrier = std::sync::Barrier::new(8);
        let accepted = std::sync::atomic::AtomicUsize::new(0);
        let refused = std::sync::atomic::AtomicUsize::new(0);

        std::thread::scope(|scope| {
            for _ in 0..8 {
                scope.spawn(|| {
                    barrier.wait();
                    match memory.append("racing", &[]) {
                        Ok(()) => {
                            accepted.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
                        }
                        Err(MemoryError::Full { .. }) => {
                            refused.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
                        }
                        Err(other) => panic!("an append failed unexpectedly: {other:?}"),
                    }
                });
            }
        });

        assert_eq!(
            accepted.load(std::sync::atomic::Ordering::SeqCst),
            1,
            "one slot is left, so exactly one racing append fits"
        );
        assert_eq!(refused.load(std::sync::atomic::Ordering::SeqCst), 7);
        assert_eq!(
            fs::read_to_string(&memory.path)
                .expect("the file is readable")
                .lines()
                .count(),
            MAX_ENTRIES,
            "concurrent appends must not push the file past its cap"
        );
    }

    #[test]
    fn the_last_entry_before_the_cap_is_accepted_and_the_next_is_refused() {
        let (_dir, memory) = memory("memory-entry-count");
        write_fixture(&memory, MAX_ENTRIES - 1);

        memory
            .append("the last one", &[])
            .expect("entry 10 000 fits");

        let error = memory
            .append("one too many", &[])
            .expect_err("append-only means no eviction at the cap");
        assert!(
            matches!(error, MemoryError::Full { max: 10_000 }),
            "{error:?}"
        );
        assert_eq!(
            fs::read_to_string(&memory.path)
                .expect("the file is readable")
                .lines()
                .count(),
            MAX_ENTRIES,
            "the refused append wrote nothing"
        );
    }

    #[test]
    fn a_corrupt_line_is_an_error_not_silently_skipped() {
        let (_dir, memory) = memory("memory-corrupt");
        write_fixture(&memory, 1);
        let mut contents = fs::read_to_string(&memory.path).expect("the fixture is readable");
        contents.push_str("not json\n");
        fs::write(&memory.path, contents).expect("the corrupt line is written");

        let error = memory
            .search("", 10)
            .expect_err("a corrupt line is surfaced, not hidden");

        assert!(
            matches!(error, MemoryError::Corrupt { line: 2 }),
            "{error:?}"
        );
    }

    #[test]
    fn a_search_before_the_first_append_is_empty_not_an_error() {
        let (_dir, memory) = memory("memory-absent");

        assert!(memory
            .search("anything", 10)
            .expect("an absent file holds no entries")
            .is_empty());
        assert!(!memory.path.exists());
    }

    #[test]
    fn a_list_is_newest_first_and_can_name_one_tag() {
        let (_dir, memory) = memory("memory-list");
        memory
            .append("the first", &["ops".to_owned()])
            .expect("append");
        memory
            .append("the second", &["ops".to_owned(), "db".to_owned()])
            .expect("append");
        memory
            .append("the third", &["db".to_owned()])
            .expect("append");

        let all: Vec<String> = memory
            .list(10, None)
            .expect("the list reads")
            .into_iter()
            .map(|entry| entry.text)
            .collect();
        assert_eq!(all, ["the third", "the second", "the first"]);

        let ops: Vec<String> = memory
            .list(10, Some("ops"))
            .expect("the list reads")
            .into_iter()
            .map(|entry| entry.text)
            .collect();
        assert_eq!(
            ops,
            ["the second", "the first"],
            "the tag is matched exactly, not as a prefix"
        );

        assert_eq!(memory.list(2, None).expect("the list reads").len(), 2);
        assert!(matches!(
            memory.list(MAX_RESULTS + 1, None),
            Err(MemoryError::LimitTooLarge { .. })
        ));
    }

    #[test]
    fn an_export_is_the_stores_own_bytes() {
        let (_dir, memory) = memory("memory-export");
        memory.append("one", &[]).expect("append");
        memory.append("two", &[]).expect("append");

        let export = memory.export(4096).expect("the export reads");
        assert_eq!(export.entries, 2);
        assert!(!export.truncated);
        assert_eq!(export.text.lines().count(), 2);
        assert!(export.text.contains("\"one\""));
        assert_eq!(export.total_bytes, export.text.len());
    }

    #[test]
    fn an_export_cut_by_the_cap_ends_on_a_line_boundary() {
        let (_dir, memory) = memory("memory-export-cut");
        memory.append("one", &[]).expect("append");
        memory.append("two", &[]).expect("append");
        let whole = memory.export(4096).expect("the export reads");

        let first_line = whole.text.lines().next().expect("a line").len();
        let export = memory
            .export(first_line + 1)
            .expect("a cap inside the second line still exports the first");
        assert!(export.truncated);
        assert_eq!(export.entries, 1);
        assert_eq!(export.text.lines().count(), 1);
        assert_eq!(export.total_bytes, whole.text.len());
    }
}
