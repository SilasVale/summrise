//! THE MAIL WORKER'S DECISIONS, DRIVEN ON THE HOST.
//!
//! `lib.rs` is the half that compiles without wasm — path routing, the constant-time secret compare, ids, the
//! object keys and the index rows. `worker.rs` (Task 3) is the half that only exists on `wasm32`, and it is not
//! tested here because `#[event(fetch)]` expands to nothing on the host; the local `wrangler dev` run in Task 3
//! is what drives those two handlers end to end.
//!
//! WHAT EVERY TEST HERE IS FOR: the read path is protected by one unguessable path segment, so this file is
//! where "a near miss is a 404" is a fact rather than an intention, and where the object keys are pinned so a
//! message can never be written over another.

use summrise_mail_worker::{
    date_partition, idx_key, index_json, new_id, parse_index_row, parse_route, raw_key, render_index,
    secret_eq, IndexRow, NewRow, Route, DEFAULT_INDEX_LIMIT, ID_HEX_LEN, MAX_INDEX_LIMIT,
};

const SECRET: &str = "s3cret-not-in-any-url";

// ── the read path ────────────────────────────────────────────────────────────────────────────────────────

#[test]
fn a_well_formed_index_path_parses_with_the_default_limit() {
    assert_eq!(
        parse_route("/s/s3cret-not-in-any-url/", SECRET),
        Some(Route::Index {
            limit: DEFAULT_INDEX_LIMIT
        })
    );
}

#[test]
fn a_well_formed_message_path_parses_to_its_id() {
    let id = "0123456789abcdef0123456789abcdef";
    assert_eq!(
        parse_route(&format!("/s/s3cret-not-in-any-url/m/{id}"), SECRET),
        Some(Route::Message { id: id.to_string() })
    );
    // The query is not part of the message route, and it must not make it a miss either.
    assert_eq!(
        parse_route(&format!("/s/s3cret-not-in-any-url/m/{id}?x=1"), SECRET),
        Some(Route::Message { id: id.to_string() })
    );
}

#[test]
fn a_near_miss_secret_is_refused() {
    for path in [
        "/s/s3cret-not-in-any-urL/",   // one byte different, same length
        "/s/s3cret-not-in-any-url",    // no trailing slash
        "/s/s3cret/",                  // the secret's prefix is not the secret
        "/s/s3cret-not-in-any-urlx/",  // the secret plus a byte
        "/s//",                        // nothing where the secret goes
        "/s/s3cret-not-in-any-url/m",  // a message path with no id
        "/s3cret-not-in-any-url/",     // the secret without its /s/ segment
        "/",                           // the root
        "/s/s3cret-not-in-any-url/m/0123456789ABCDEF0123456789ABCDEF", // an uppercase id is not an id
        "/s/s3cret-not-in-any-url/m/0123456789abcdef0123456789abcde",  // 31 bytes
        "/s/s3cret-not-in-any-url/m/0123456789abcdef0123456789abcdeff", // 33 bytes
        "/s/s3cret-not-in-any-url/m/0123456789abcdef0123456789abcdeg", // a non-hex byte
        "/s/s3cret-not-in-any-url/m/0123456789abcdef0123456789abcdef/extra",
    ] {
        assert_eq!(parse_route(path, SECRET), None, "path: {path}");
    }
}

#[test]
fn a_secret_of_the_right_length_but_wrong_bytes_is_refused() {
    // Same length as the real secret, differing in the last byte — the shape a timing-safe compare exists for.
    assert_eq!(parse_route("/s/s3cret-not-in-any-urX/", SECRET), None);
    assert!(secret_eq(SECRET, SECRET));
    assert!(!secret_eq(SECRET, "s3cret-not-in-any-urX"));
    assert!(!secret_eq("short", "longer-than-short"));
    assert!(secret_eq("", ""));
}

#[test]
fn an_empty_secret_refuses_every_path_rather_than_failing_open() {
    // An unset MAIL_PATH_SECRET is a configuration error, and the failure mode this rules out is the whole
    // read path answering to whoever finds the hostname: with an empty secret, `/s//` would otherwise match.
    assert_eq!(parse_route("/s//", ""), None);
    assert_eq!(parse_route("/s/x/", ""), None);
    assert_eq!(parse_route("/s//m/0123456789abcdef0123456789abcdef", ""), None);
}

#[test]
fn only_the_documented_limit_query_is_honoured_and_it_is_capped() {
    let at = |q: &str| parse_route(&format!("/s/{SECRET}/?{q}"), SECRET);
    assert_eq!(at("n=5"), Some(Route::Index { limit: 5 }));
    assert_eq!(at("n=50"), Some(Route::Index { limit: MAX_INDEX_LIMIT }));
    assert_eq!(at("n=999"), Some(Route::Index { limit: MAX_INDEX_LIMIT }));
    assert_eq!(at("n=0"), Some(Route::Index { limit: DEFAULT_INDEX_LIMIT }));
    assert_eq!(at("n=-1"), Some(Route::Index { limit: DEFAULT_INDEX_LIMIT }));
    assert_eq!(at("n=x"), Some(Route::Index { limit: DEFAULT_INDEX_LIMIT }));
    assert_eq!(at("other=5"), Some(Route::Index { limit: DEFAULT_INDEX_LIMIT }));
    assert_eq!(at("n=7&other=5"), Some(Route::Index { limit: 7 }));
}

// ── ids and keys ─────────────────────────────────────────────────────────────────────────────────────────

#[test]
fn ids_are_lowercase_hex_of_the_required_length() {
    let id = new_id(&[0xab; 16]);
    assert_eq!(id, "abababababababababababababababab");
    assert_eq!(id.len(), ID_HEX_LEN);
    assert_eq!(new_id(&[0x00; 16]), "00000000000000000000000000000000");
    assert_eq!(new_id(&[0x0f; 16]), "0f0f0f0f0f0f0f0f0f0f0f0f0f0f0f0f");
}

#[test]
fn keys_partition_by_date_and_id() {
    assert_eq!(raw_key("2026-10-14", "ab"), "mail/raw/2026-10-14/ab.eml");
    assert_eq!(idx_key("ab"), "mail/idx/ab.json");
    // Two messages in the same second must not collide: different ids, different objects.
    assert_ne!(raw_key("2026-10-14", "a"), raw_key("2026-10-14", "b"));
    // ...and the same id in a different partition is a different object, which is why the id alone is not the
    // raw key.
    assert_ne!(raw_key("2026-10-14", "a"), raw_key("2026-10-15", "a"));
}

#[test]
fn a_raw_date_header_is_never_used_as_the_partition() {
    // The partition comes from the worker's clock, formatted as ISO 8601. A sender-supplied RFC 2822 `Date`
    // header must never reach a key: it is attacker-controlled text.
    assert_eq!(date_partition("2026-10-14T05:57:00.000Z"), "2026-10-14");
    assert_eq!(date_partition("2026-10-14T05:57:00Z"), "2026-10-14");
    assert_eq!(date_partition("Tue, 27 Aug 2024 08:49:44 -0700"), "undated");
    assert_eq!(date_partition(""), "undated");
    assert_eq!(date_partition("2026-10-14"), "undated");
    assert_eq!(date_partition("../../etc/passwd"), "undated");
}

// ── the index rows ───────────────────────────────────────────────────────────────────────────────────────

fn sample_new_row<'a>() -> NewRow<'a> {
    NewRow {
        id: "11111111111111111111111111111111",
        from: "operator@example.com",
        to: "dsh@example.com",
        subject: "hello",
        date: "Tue, 27 Aug 2024 08:49:44 -0700",
        bytes: 1234,
        raw: "mail/raw/2026-10-14/11111111111111111111111111111111.eml",
    }
}

#[test]
fn an_index_row_round_trips_through_json() {
    let json = index_json(&sample_new_row());
    let row = parse_index_row(&json, "11111111111111111111111111111111", "2026-10-14T05:57:00.000Z")
        .expect("a row we wrote must parse");
    assert_eq!(
        row,
        IndexRow {
            id: "11111111111111111111111111111111".to_string(),
            uploaded: "2026-10-14T05:57:00.000Z".to_string(),
            from: "operator@example.com".to_string(),
            to: "dsh@example.com".to_string(),
            subject: "hello".to_string(),
            date: "Tue, 27 Aug 2024 08:49:44 -0700".to_string(),
            bytes: 1234,
            raw: "mail/raw/2026-10-14/11111111111111111111111111111111.eml".to_string(),
        }
    );
}

#[test]
fn a_row_with_missing_headers_parses_to_empty_strings_rather_than_failing() {
    // Subject, Date and Message-ID are all optional in email, so a row can legitimately carry empty strings —
    // and an object that predates a field, or that someone wrote by hand, must not take the index down.
    let row = parse_index_row("{}", "abc", "2026-10-14T05:57:00.000Z").expect("an empty object still parses");
    assert_eq!(row.from, "");
    assert_eq!(row.subject, "");
    assert_eq!(row.date, "");
    assert_eq!(row.bytes, 0);
    assert_eq!(row.raw, "");
    // The id and the upload time come from the listing, not from the object, so they survive an empty body.
    assert_eq!(row.id, "abc");
    assert_eq!(row.uploaded, "2026-10-14T05:57:00.000Z");

    assert_eq!(parse_index_row("not json at all", "abc", "x"), None);
}

#[test]
fn render_index_lists_newest_first_with_the_fields_the_agent_reads() {
    let rows = vec![
        IndexRow {
            id: "11111111111111111111111111111111".to_string(),
            uploaded: "2026-10-14T05:57:00.000Z".to_string(),
            from: "operator@example.com".to_string(),
            to: "dsh@example.com".to_string(),
            subject: "hello".to_string(),
            date: "Tue, 27 Aug 2024 08:49:44 -0700".to_string(),
            bytes: 1234,
            raw: "mail/raw/2026-10-14/11111111111111111111111111111111.eml".to_string(),
        },
        IndexRow {
            id: "22222222222222222222222222222222".to_string(),
            uploaded: "2026-10-13T09:00:00.000Z".to_string(),
            from: "".to_string(),
            to: "dsh@example.com".to_string(),
            subject: "".to_string(),
            date: "".to_string(),
            bytes: 42,
            raw: "mail/raw/2026-10-13/22222222222222222222222222222222.eml".to_string(),
        },
    ];
    assert_eq!(
        render_index(&rows),
        "mail index: 2 message(s), newest first\n\
         2026-10-14T05:57:00.000Z  operator@example.com  hello  1234 B  11111111111111111111111111111111\n\
         2026-10-13T09:00:00.000Z  (unknown sender)  (no subject)  42 B  22222222222222222222222222222222\n"
    );
    assert_eq!(render_index(&[]), "mail index: 0 message(s), newest first\n");
}
