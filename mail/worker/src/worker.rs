//! THE WASM-ONLY HALF — the two events. Everything that can be decided without a runtime lives in `lib.rs`,
//! where `cargo test` reaches it; this file is I/O and the order the I/O happens in.
//!
//! WHY THE ORDER IS THE DESIGN, IN BOTH HANDLERS:
//!
//!   `email()`  **forwards first**, then stores. Cloudflare routes one address pattern to ONE destination, so
//!              this worker is the only thing standing between the sender and the operator's inbox. If the
//!              store fails after the forward, the operator still has the message in 163 and can ask why the
//!              index is missing it; if the forward failed after a successful store, the loss would be silent
//!              from both ends.
//!
//!   `fetch()`  reads the index row to find the raw object's key, because the key's date partition is the
//!              worker's RECEIVE time and cannot be rebuilt from anything in the message. One extra `get` per
//!              message read, and it buys a key a sender cannot steer.
//!
//! NO MESSAGE IS EVER BUFFERED INTO THE ISOLATE: `message.raw()` is a `ReadableStream` and `R2Bucket::put`
//! takes one (`Data::ReadableStream`, the same shape `relay/worker` uses for request bodies). Workers Free
//! allows 10 ms of CPU per request and this worker spends none of it on the message bytes.

use crate::{
    date_partition_from_millis, idx_key, index_json, new_id_from, parse_index_row, parse_route, raw_key,
    render_index, NewRow, Route,
};
use worker::*;

/// Where the copy goes. Empty means "do not forward", which is what local development sets so that a
/// `wrangler dev` run cannot depend on a verified destination address.
const FORWARD_TO_VAR: &str = "FORWARD_TO";

/// The one path segment that protects the read side. A secret rather than a var, because a var's value is in
/// the repository.
const SECRET_VAR: &str = "MAIL_PATH_SECRET";

/// The R2 binding, as `mail/wrangler.jsonc` declares it.
const BUCKET: &str = "MAIL";

/// Where the index rows live. Every object under it is a message; the raw bytes live under `mail/raw/`.
const IDX_PREFIX: &str = "mail/idx/";

/// How many objects the listing asks for. Larger than any window of mail could hold, because the read path's
/// real cap is the SUBREQUEST budget (see `MAX_INDEX_LIMIT`), not the number of messages a week brings.
const LIST_LIMIT: u32 = 1_000;

/// `GET` only, and nothing else answers. A wrong secret is a 404 with no body rather than a 403, so the read
/// host tells an unauthenticated caller nothing at all — not even that it exists.
#[event(fetch)]
pub async fn fetch(req: Request, env: Env, _ctx: Context) -> Result<Response> {
    let secret = env.var(SECRET_VAR).map(|v| v.to_string()).unwrap_or_default();
    let url = req.url()?;
    let path_and_query = format!("{}?{}", url.path(), url.query().unwrap_or(""));
    let Some(route) = parse_route(&path_and_query, &secret) else {
        return not_found();
    };
    let bucket = env.bucket(BUCKET)?;
    match route {
        Route::Index { limit } => index_response(&bucket, limit).await,
        Route::Message { id } => message_response(&bucket, &id).await,
    }
}

/// `GET /s/<secret>/` — what arrived, newest first.
async fn index_response(bucket: &Bucket, limit: usize) -> Result<Response> {
    let listing = bucket.list().prefix(IDX_PREFIX).limit(LIST_LIMIT).execute().await?;
    // Newest first, by the object's own upload time. `worker::Date`'s `Display` is JavaScript's
    // `toString()`, so the sort key is milliseconds and the printed stamp is built from them in `lib.rs`.
    let mut metas: Vec<(u64, String)> = listing
        .objects()
        .iter()
        .map(|object| (object.uploaded().as_millis(), object.key()))
        .collect();
    metas.sort_by_key(|entry| std::cmp::Reverse(entry.0));
    metas.truncate(limit);

    let mut rows = Vec::with_capacity(metas.len());
    for (millis, key) in metas {
        let id = id_from_key(&key);
        let Some(object) = bucket.get(key).execute().await? else {
            continue;
        };
        let Some(body) = object.body() else {
            continue;
        };
        let json = Response::from_body(body.response_body()?)?.text().await?;
        // A row we wrote always parses; an object that does not is not ours to explain, and it must not take
        // the whole index down with it.
        if let Some(row) = parse_index_row(&json, &id, &crate::stamp_from_millis(millis)) {
            rows.push(row);
        }
    }
    Ok(Response::ok(render_index(&rows))?.with_headers(headers("text/plain; charset=utf-8")?))
}

/// `GET /s/<secret>/m/<id>` — one message's original bytes.
async fn message_response(bucket: &Bucket, id: &str) -> Result<Response> {
    let Some(row_object) = bucket.get(idx_key(id)).execute().await? else {
        return not_found();
    };
    let Some(body) = row_object.body() else {
        return not_found();
    };
    let json = Response::from_body(body.response_body()?)?.text().await?;
    let Some(row) = parse_index_row(&json, id, "") else {
        return not_found();
    };
    if row.raw.is_empty() {
        return not_found();
    }
    let Some(object) = bucket.get(row.raw).execute().await? else {
        return not_found();
    };
    let Some(body) = object.body() else {
        return not_found();
    };
    Ok(Response::from_body(body.response_body()?)?.with_headers(headers("message/rfc822")?))
}

/// The last path segment of `mail/idx/<id>.json`. The id is the object's name, which is why it is not also
/// stored inside the row.
fn id_from_key(key: &str) -> String {
    key.trim_start_matches(IDX_PREFIX)
        .trim_end_matches(".json")
        .to_string()
}

fn headers(content_type: &str) -> Result<Headers> {
    let headers = Headers::new();
    headers.set("content-type", content_type)?;
    // A mail body is not a document to be cached or sniffed.
    headers.set("cache-control", "no-store")?;
    headers.set("x-content-type-options", "nosniff")?;
    Ok(headers)
}

fn not_found() -> Result<Response> {
    Response::empty().map(|response| response.with_status(404))
}

/// `email()` — the inbound message.
#[event(email)]
pub async fn email(message: ForwardableEmailMessage, env: Env, _ctx: Context) -> Result<()> {
    let secret = env.var(SECRET_VAR).map(|v| v.to_string()).unwrap_or_default();
    if secret.is_empty() {
        // The read path refuses everything without a secret, and an id derived from an empty one would be
        // derivable by anyone. Failing here is loud in the worker's logs, and the forward below has already
        // put the message in front of the operator by the time this can happen — except in this first case,
        // which is therefore checked BEFORE the forward so that the failure is not silent at both ends.
        return Err(Error::RustError(format!("{SECRET_VAR} is not configured")));
    }

    let forward_to = env.var(FORWARD_TO_VAR).map(|v| v.to_string()).unwrap_or_default();
    if !forward_to.is_empty() {
        message.forward(&forward_to).await?;
    }

    let millis = Date::now().as_millis();
    let size = message.raw_size() as u64;
    let (from, to) = (message.from(), message.to());
    let id = new_id_from(&secret, millis, size, &from, &to);
    let raw = raw_key(&date_partition_from_millis(millis), &id);

    let bucket = env.bucket(BUCKET)?;
    // **STREAMED, BUT NOT AS A BARE `ReadableStream` — MEASURED THE HARD WAY.** `R2Bucket::put` refuses a
    // stream whose length it does not know: "TypeError: Provided readable stream must have a known length
    // (request/response body or readable half of FixedLengthStream)". A mail message's stream has no known
    // length — except that the runtime hands us one in `rawSize`, so the message goes through a
    // `FixedLengthStream` and still never lands in the isolate's memory. The relay can pass the bare stream
    // because a request body already carries its length.
    let sized = FixedLengthStream::wrap(ByteStream::from(message.raw()), size);
    bucket.put(raw.clone(), Data::Stream(sized)).execute().await?;

    let headers = message.headers();
    let header = |name: &str| headers.get(name).ok().flatten().unwrap_or_default();
    let subject = header("subject");
    let date = header("date");
    let row = NewRow {
        id: &id,
        from: &from,
        to: &to,
        subject: &subject,
        date: &date,
        bytes: size,
        raw: &raw,
    };
    bucket.put(idx_key(&id), index_json(&row)).execute().await?;
    Ok(())
}
