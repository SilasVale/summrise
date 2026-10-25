//! The journal-backed memory capability (design §4.5).
//!
//! One JSON object per line under `<data>/memory.jsonl`. A line is one operation in a journal —
//! `append`, `update` or `delete` — and the live entries are what a reverse replay of those
//! operations produces. The file stays append-only, so an earlier byte is never rewritten.
//!
//! A line written before the journal carries only `{"text","tags"}` and is read as an append whose
//! id is derived from its own bytes, and from its ordinal when two lines are byte-identical. An old
//! file therefore keeps working without a migration, and a duplicate stays two distinguishable
//! entries instead of collapsing into one.
//!
//! The caps are part of the capability, not a client convention: **1 MiB per serialized operation
//! line** (tags and JSON escaping included, the trailing newline not counted), 32 tags, 128 bytes
//! per tag, 10 000 entries, 100 results. The line cap is a storage cap, not a text-length cap: the
//! check happens after `serde_json` writes the line, so a quote-heavy 1 MiB text cannot serialize to
//! two megabytes on disk.
//!
//! A read walks the journal backwards, newest line first, keeping two sets: ids already emitted, and
//! ids a delete has tombstoned. That is what makes an update appear at the update's position rather
//! than at the append's, and it lets one pass return newest-first without holding the journal in
//! memory.
//!
//! The MCP surface serves several sessions at once, so one lock makes the file's invariants hold
//! under concurrency: an append's count-then-write is one critical section (two racing appends
//! cannot both take the last slot), and a read takes the read side, so it never reads a line that is
//! halfway written.

use std::collections::{HashMap, HashSet};
use std::fmt;
use std::fs::{self, File, OpenOptions};
use std::io::{self, BufRead, BufReader, Read, Seek, SeekFrom, Write};
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

/// The largest serialized operation line: tags, the id, JSON quoting and escapes included; the
/// newline is extra.
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

/// What `export` returns: the live entries as JSONL, how many whole lines they are, and whether the
/// cap cut the store.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Export {
    /// The live entries, newest first, ending on a newline unless the store is empty.
    pub text: String,
    /// Whole lines in [`Export::text`].
    pub entries: usize,
    /// The live entries were larger than the cap and `text` is a prefix.
    pub truncated: bool,
    /// The size of all live entries, so a caller can see what it did not get.
    pub total_bytes: usize,
}

/// One remembered fact, and the name a later operation uses to refer to it.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Entry {
    /// The entry's name: minted by an append, derived for a legacy line. It belongs to the journal
    /// record rather than to the payload, so an append's `entry` object does not repeat it.
    #[serde(default)]
    pub id: String,
    /// The entry's text.
    pub text: String,
    /// Free-form tags; a search matches them as well as the text.
    pub tags: Vec<String>,
}

/// What a journal line does to the entry it names.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Op {
    /// A new entry.
    Append,
    /// A new version of an existing entry, at this line's position in the journal.
    Update,
    /// A tombstone: the named entry is no longer live.
    Delete,
}

/// One line of the journal.
///
/// The id is at this level, not inside `entry`, because it is what the operation names; the payload
/// keeps the pre-journal `{"text","tags"}` shape. An append or update carries an id and an entry, a
/// delete carries an id alone.
#[derive(Clone, Debug, PartialEq, Eq, Deserialize)]
pub struct Record {
    /// What the line does.
    pub op: Op,
    /// The entry the line names.
    pub id: Option<String>,
    /// The entry an append or update writes; a delete carries none.
    pub entry: Option<Entry>,
}

/// The entry half of a journal line: the record already carries the id, so the payload omits it.
#[derive(Serialize)]
struct Payload<'a> {
    text: &'a str,
    tags: &'a [String],
}

// Serde's `tag` container attribute is an enum attribute; on a struct it emits a second `op` key.
// This impl is what keeps the record's shape the design names: op, then the optional id, then the
// optional payload, with the payload's id only at the record level.
impl Serialize for Record {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        use serde::ser::SerializeStruct;

        let mut record = serializer.serialize_struct("Record", 3)?;
        record.serialize_field("op", &self.op)?;
        if let Some(id) = &self.id {
            record.serialize_field("id", id)?;
        }
        if let Some(entry) = &self.entry {
            record.serialize_field(
                "entry",
                &Payload {
                    text: &entry.text,
                    tags: &entry.tags,
                },
            )?;
        }
        record.end()
    }
}

/// Why an append or a search was refused or failed.
#[derive(Debug)]
pub enum MemoryError {
    /// The serialized operation line exceeds [`MAX_ENTRY_BYTES`].
    TooLarge {
        /// The line's size in bytes, or the raw payload's before serialization.
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

    /// Append one entry and return it, id included.
    ///
    /// Validation happens before the write lock and before the serialized line is built where it
    /// can: the tag count and each tag's length first, then the JSON serialization, and finally the
    /// stored line's own length. The cap is on what is written, not on `text.len()`, because tags,
    /// escaping, the id and the journal's envelope are all part of the line.
    pub fn append(&self, text: &str, tags: &[String]) -> Result<Entry, MemoryError> {
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
            id: mint_id(),
            text: text.to_owned(),
            tags: tags.to_vec(),
        };
        let record = Record {
            op: Op::Append,
            id: Some(entry.id.clone()),
            entry: Some(entry.clone()),
        };
        let mut line = serde_json::to_string(&record)
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
        file.flush().map_err(MemoryError::Io)?;
        Ok(entry)
    }

    /// Walk the journal backwards and hand every live entry to `emit`, newest first.
    ///
    /// Reverse order is the replay: an update is later in the file than the append it replaces, so
    /// the walk meets the update first and `seen` skips the append. `deleted` remembers tombstones,
    /// which are newer than any line they can name. Both sets hold ids, not entries, so the walk's
    /// memory is the line index plus the names the journal mentions.
    fn replay(&self, mut emit: impl FnMut(Entry)) -> Result<(), MemoryError> {
        let _reading = self
            .lock
            .read()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let mut file = match File::open(&self.path) {
            Ok(file) => file,
            Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(()),
            Err(error) => return Err(MemoryError::Io(error)),
        };
        let lines = index(&mut file)?;
        let mut seen: HashSet<String> = HashSet::new();
        let mut deleted: HashSet<String> = HashSet::new();
        let mut bytes = Vec::new();
        for line in lines.iter().rev() {
            bytes.resize(line.len, 0);
            file.seek(SeekFrom::Start(line.offset))
                .map_err(MemoryError::Io)?;
            file.read_exact(&mut bytes).map_err(MemoryError::Io)?;
            match parse_line(&bytes, line.number, line.occurrence)? {
                Parsed::Record(record) => {
                    let id = record
                        .id
                        .ok_or(MemoryError::Corrupt { line: line.number })?;
                    match record.op {
                        Op::Append | Op::Update => {
                            let mut entry = record
                                .entry
                                .ok_or(MemoryError::Corrupt { line: line.number })?;
                            if seen.contains(&id) || deleted.contains(&id) {
                                continue;
                            }
                            entry.id = id.clone();
                            seen.insert(id);
                            emit(entry);
                        }
                        Op::Delete => {
                            deleted.insert(id);
                        }
                    }
                }
                Parsed::Legacy(entry) => {
                    if seen.contains(&entry.id) || deleted.contains(&entry.id) {
                        continue;
                    }
                    seen.insert(entry.id.clone());
                    emit(entry);
                }
            }
        }
        Ok(())
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

        let needle = query.to_lowercase();
        let mut found = Vec::new();
        self.replay(|entry| {
            if found.len() >= limit {
                return;
            }
            if entry.text.to_lowercase().contains(&needle)
                || entry
                    .tags
                    .iter()
                    .any(|tag| tag.to_lowercase().contains(&needle))
            {
                found.push(entry);
            }
        })?;
        Ok(found)
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

        let wanted = tag.map(str::to_lowercase);
        let mut found = Vec::new();
        self.replay(|entry| {
            if found.len() >= limit {
                return;
            }
            if let Some(wanted) = &wanted {
                if !entry.tags.iter().any(|tag| tag.to_lowercase() == *wanted) {
                    return;
                }
            }
            found.push(entry);
        })?;
        Ok(found)
    }

    /// The live entries as JSONL, cut at a line boundary when they are larger than `max_bytes`.
    ///
    /// The export is what the device remembers, not the journal that stores it: an update appears as
    /// the updated entry and a tombstoned entry is absent. The cap exists because a tool result is a
    /// JSON-RPC frame, not a stream: a store at its 10 000-entry ceiling can be gigabytes, and a
    /// client that asked for everything can ask again with the byte offset the truncation names.
    pub fn export(&self, max_bytes: usize) -> Result<Export, MemoryError> {
        let mut text = String::new();
        let mut entries = 0;
        let mut total_bytes = 0;
        let mut truncated = false;
        let mut failure = None;
        self.replay(|entry| {
            if failure.is_some() {
                return;
            }
            let mut line = match serde_json::to_string(&entry) {
                Ok(line) => line,
                Err(error) => {
                    failure = Some(MemoryError::Io(io::Error::new(
                        io::ErrorKind::InvalidData,
                        error,
                    )));
                    return;
                }
            };
            line.push('\n');
            total_bytes += line.len();
            // Whole lines only: one line that does not fit ends the export, so a client never has
            // to guess whether the last line was cut in the middle.
            if !truncated && text.len() + line.len() <= max_bytes {
                text.push_str(&line);
                entries += 1;
            } else {
                truncated = true;
            }
        })?;
        if let Some(error) = failure {
            return Err(error);
        }
        Ok(Export {
            text,
            entries,
            truncated,
            total_bytes,
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

/// One non-empty line's place in the journal.
struct LineRef {
    /// Byte offset of the line's first byte.
    offset: u64,
    /// The line's length without its newline.
    len: usize,
    /// 1-based line number in the file, blank lines included, for [`MemoryError::Corrupt`].
    number: usize,
    /// Which byte-identical copy of the line this is, 1-based; only a legacy id uses it.
    occurrence: usize,
}

/// One parsed journal line: an operation, or a bare line from before operations existed.
enum Parsed {
    Record(Record),
    /// A pre-journal entry, carrying the id its bytes and ordinal derive.
    Legacy(Entry),
}

/// Index the journal in one forward pass.
///
/// A read replays newest-first, but a legacy line's id depends on how many byte-identical copies
/// came before it; the index is that forward count plus the offset each line is read from. It is
/// small and fixed-size per line, not the line itself, so a store near its ceiling does not have to
/// be held in memory to be read.
fn index(file: &mut File) -> Result<Vec<LineRef>, MemoryError> {
    let mut reader = BufReader::new(file);
    let mut lines = Vec::new();
    let mut occurrences: HashMap<[u8; 8], usize> = HashMap::new();
    let mut offset = 0u64;
    let mut number = 0usize;
    loop {
        let mut line = Vec::new();
        let read = reader
            .read_until(b'\n', &mut line)
            .map_err(MemoryError::Io)?;
        if read == 0 {
            break;
        }
        number += 1;
        let start = offset;
        offset += read as u64;
        if line.last() == Some(&b'\n') {
            line.pop();
        }
        if line.last() == Some(&b'\r') {
            line.pop();
        }
        if line.iter().all(u8::is_ascii_whitespace) {
            continue;
        }
        let digest = Sha256::digest(&line);
        let mut prefix = [0u8; 8];
        prefix.copy_from_slice(&digest[..8]);
        let occurrence = occurrences.entry(prefix).or_insert(0);
        *occurrence += 1;
        lines.push(LineRef {
            offset: start,
            len: line.len(),
            number,
            occurrence: *occurrence,
        });
    }
    Ok(lines)
}

/// Read one line as the journal defines it.
///
/// A bare `{"text","tags"}` object is the format before the journal and stays readable. A line that
/// names an operation must parse as that operation, so an unknown op is corruption rather than a
/// legacy line whose extra field a permissive parser would ignore.
fn parse_line(bytes: &[u8], number: usize, occurrence: usize) -> Result<Parsed, MemoryError> {
    #[derive(Deserialize)]
    struct Probe {
        #[serde(default)]
        op: Option<Op>,
    }

    let probe: Probe =
        serde_json::from_slice(bytes).map_err(|_| MemoryError::Corrupt { line: number })?;
    if probe.op.is_some() {
        let record: Record =
            serde_json::from_slice(bytes).map_err(|_| MemoryError::Corrupt { line: number })?;
        return Ok(Parsed::Record(record));
    }

    let mut entry: Entry =
        serde_json::from_slice(bytes).map_err(|_| MemoryError::Corrupt { line: number })?;
    let line = std::str::from_utf8(bytes).map_err(|_| MemoryError::Corrupt { line: number })?;
    entry.id = legacy_id(line, occurrence);
    Ok(Parsed::Legacy(entry))
}

/// A legacy line's id: the first 16 hex of its sha256, plus its occurrence when lines repeat.
///
/// The derivation is over the line's own bytes, so one file derives the same ids on every read. A
/// byte-identical duplicate is a different entry and gets a suffix instead of collapsing into the
/// first one, which is what lets each be named and retired separately.
fn legacy_id(line: &str, occurrence: usize) -> String {
    let digest = Sha256::digest(line.as_bytes());
    let mut id = hex(&digest[..8]);
    if occurrence > 1 {
        id.push_str(&format!("-{occurrence}"));
    }
    id
}

/// 8 bytes from the operating system's generator, as hex.
///
/// An id is a name, not a secret, but it must not repeat: 64 random bits keep a collision
/// negligible even at the 10 000-entry ceiling.
fn mint_id() -> String {
    use rand_core::{OsRng, RngCore};

    let mut bytes = [0u8; 8];
    OsRng.fill_bytes(&mut bytes);
    hex(&bytes)
}

/// Lower-case hex, the form ids are written in.
fn hex(bytes: &[u8]) -> String {
    let mut text = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        text.push_str(&format!("{byte:02x}"));
    }
    text
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

        // A quote-light text leaves room for the journal envelope, so its serialized line fits.
        let text = "x".repeat(MAX_ENTRY_BYTES - 128);
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
                        Ok(_) => {
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
    fn an_export_is_the_live_entries_as_jsonl() {
        let (_dir, memory) = memory("memory-export");
        memory.append("one", &[]).expect("append");
        memory.append("two", &[]).expect("append");

        let export = memory.export(4096).expect("the export reads");
        assert_eq!(export.entries, 2);
        assert!(!export.truncated);
        assert_eq!(export.text.lines().count(), 2);
        assert!(export.text.contains("\"text\":\"one\""));
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

    #[test]
    fn legacy_lines_keep_working_with_stable_distinct_ids() {
        let (_dir, memory) = memory("memory-legacy-ids");
        let first = json_line("the first fact", &["legacy"]);
        let second = json_line("the second fact", &["legacy"]);
        let parent = memory.path.parent().expect("the data directory");
        fs::create_dir_all(parent).expect("the data directory is creatable");
        fs::write(&memory.path, format!("{first}\n{second}\n{second}\n"))
            .expect("the legacy fixture is written");

        let entries = memory.search("", 10).expect("the legacy file is readable");
        let texts: Vec<&str> = entries.iter().map(|entry| entry.text.as_str()).collect();
        assert_eq!(
            texts,
            ["the second fact", "the second fact", "the first fact"],
            "a duplicate is two entries, and the file is read newest-first"
        );

        let ids: Vec<&str> = entries.iter().map(|entry| entry.id.as_str()).collect();
        let mut distinct = ids.clone();
        distinct.sort_unstable();
        distinct.dedup();
        assert_eq!(distinct.len(), 3, "three entries have three ids: {ids:?}");
        assert_eq!(
            entries[0].id,
            format!("{}-2", entries[1].id),
            "the duplicate's id is the base derived id plus its occurrence"
        );
        assert_eq!(entries[1].id.len(), 16);
        assert!(entries[1].id.bytes().all(|byte| byte.is_ascii_hexdigit()));
        assert_ne!(entries[1].id, entries[2].id);

        let again = memory.search("", 10).expect("a second read");
        let again: Vec<&str> = again.iter().map(|entry| entry.id.as_str()).collect();
        assert_eq!(
            ids, again,
            "the derivation is over the line's bytes, so a second read derives the same ids"
        );

        let listed = memory
            .list(10, None)
            .expect("the list reads the same journal");
        let listed: Vec<&str> = listed.iter().map(|entry| entry.id.as_str()).collect();
        assert_eq!(ids, listed, "search and list replay the same ids");
    }

    #[test]
    fn an_append_returns_the_id_it_wrote_and_does_not_rewrite_the_legacy_line() {
        let (_dir, memory) = memory("memory-append-id");
        let legacy = json_line("before the journal", &[]);
        let parent = memory.path.parent().expect("the data directory");
        fs::create_dir_all(parent).expect("the data directory is creatable");
        fs::write(&memory.path, format!("{legacy}\n")).expect("the legacy fixture is written");

        let appended = memory
            .append("after the journal", &["new".to_owned()])
            .expect("the append");
        assert_eq!(appended.text, "after the journal");
        assert_eq!(appended.tags, ["new"]);
        assert_eq!(
            appended.id.len(),
            16,
            "8 random bytes are 16 hex characters"
        );
        assert!(
            appended.id.bytes().all(|byte| byte.is_ascii_hexdigit()),
            "the id is hex: {}",
            appended.id
        );

        let found = memory.search("after", 10).expect("the append is readable");
        assert_eq!(found.len(), 1);
        assert_eq!(
            found[0].id, appended.id,
            "the returned id names the stored entry"
        );

        let stored = fs::read_to_string(&memory.path).expect("the journal is readable");
        let lines: Vec<&str> = stored.lines().collect();
        assert_eq!(lines.len(), 2, "the legacy line and the appended line");
        assert_eq!(
            lines[0], legacy,
            "an append writes after the old bytes; it does not rewrite them"
        );

        let record: serde_json::Value =
            serde_json::from_str(lines[1]).expect("the appended line is JSON");
        assert_eq!(record["op"].as_str(), Some("append"));
        assert_eq!(record["id"].as_str(), Some(appended.id.as_str()));
        assert_eq!(record["entry"]["text"].as_str(), Some("after the journal"));
        assert_eq!(record["entry"]["tags"], serde_json::json!(["new"]));
        assert!(
            record["entry"].get("id").is_none(),
            "the id is the record's; the payload keeps the old entry shape"
        );

        let old: serde_json::Value =
            serde_json::from_str(lines[0]).expect("the old line still parses");
        assert_eq!(old["text"].as_str(), Some("before the journal"));
        assert!(
            old.get("op").is_none(),
            "the first line is still the bare entry the old version wrote"
        );
    }

    #[test]
    fn a_journal_replays_an_update_at_its_own_line_and_a_delete_as_a_tombstone() {
        let (_dir, memory) = memory("memory-journal-replay");
        let parent = memory.path.parent().expect("the data directory");
        fs::create_dir_all(parent).expect("the data directory is creatable");
        fs::write(
            &memory.path,
            concat!(
                "{\"op\":\"append\",\"id\":\"a\",\"entry\":{\"text\":\"old a\",\"tags\":[]}}\n",
                "{\"op\":\"append\",\"id\":\"b\",\"entry\":{\"text\":\"b\",\"tags\":[]}}\n",
                "{\"op\":\"update\",\"id\":\"a\",\"entry\":{\"text\":\"new a\",\"tags\":[]}}\n",
                "{\"op\":\"append\",\"id\":\"c\",\"entry\":{\"text\":\"c\",\"tags\":[]}}\n",
                "{\"op\":\"append\",\"id\":\"d\",\"entry\":{\"text\":\"d\",\"tags\":[]}}\n",
                "{\"op\":\"delete\",\"id\":\"d\"}\n",
            ),
        )
        .expect("the journal fixture is written");

        let entries = memory.search("", 10).expect("the journal replays");
        let texts: Vec<&str> = entries.iter().map(|entry| entry.text.as_str()).collect();
        assert_eq!(
            texts,
            ["c", "new a", "b"],
            "the update is emitted at its own line and the tombstone removes d"
        );
        assert_eq!(entries[1].id, "a");
        assert!(
            entries.iter().all(|entry| entry.id != "d"),
            "a delete names an entry that must not appear"
        );

        let listed = memory
            .list(10, None)
            .expect("the list replays the same journal");
        let texts: Vec<&str> = listed.iter().map(|entry| entry.text.as_str()).collect();
        assert_eq!(texts, ["c", "new a", "b"]);
    }

    #[test]
    fn an_export_is_the_live_entries_with_their_ids() {
        let (_dir, memory) = memory("memory-export-live");
        let legacy = json_line("a legacy fact", &[]);
        let parent = memory.path.parent().expect("the data directory");
        fs::create_dir_all(parent).expect("the data directory is creatable");
        fs::write(&memory.path, format!("{legacy}\n")).expect("the legacy fixture is written");
        let appended = memory
            .append("an appended fact", &["new".to_owned()])
            .expect("the append");

        let export = memory.export(4096).expect("the export reads");
        assert_eq!(export.entries, 2);
        assert!(!export.truncated);
        assert_eq!(export.total_bytes, export.text.len());

        let lines: Vec<serde_json::Value> = export
            .text
            .lines()
            .map(|line| serde_json::from_str(line).expect("an exported entry is JSON"))
            .collect();
        assert_eq!(lines.len(), 2);
        assert_eq!(
            lines[0]["id"].as_str(),
            Some(appended.id.as_str()),
            "the export is newest-first, like the list it backs up"
        );
        assert_eq!(
            lines[0]["text"].as_str(),
            Some("an appended fact"),
            "the export is the entry, not the journal record that stores it"
        );
        assert_eq!(lines[0]["tags"], serde_json::json!(["new"]));
        assert!(
            lines[0].get("op").is_none(),
            "the journal is an implementation detail"
        );
        assert!(
            lines[1]["id"].as_str().is_some_and(|id| id.len() == 16),
            "a legacy line exports the id derived for it"
        );
        assert_eq!(lines[1]["text"].as_str(), Some("a legacy fact"));
    }
}
