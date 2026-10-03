//! TWO PURE HELPERS FROM `gateway/src/tooling.ts`.
//!
//! `probeEnvKeyName` is a table lookup with a DEFAULT (an unknown channel probes DeepSeek's key rather
//! than nothing), and `encodeBase64Utf8` is the base64 the installer scripts embed. Both are pure; the
//! rest of that module reads KV, R2 and upstreams.

/// `PROBE_ENV_KEYS` — the worker-level key each channel's probe uses.
const PROBE_ENV_KEYS: [(&str, &str); 8] = [
    ("or", "OPENROUTER_API_KEY"),
    ("qw", "QWEN_API_KEY"),
    ("nv", "NVAPI_KEY"),
    ("gmi", "GMI_API_KEY"),
    ("cm", "CMD_API_KEY"),
    ("amd", "AMD_API_KEY"),
    ("r4", "R4_API_KEY"),
    ("ds", "DEEPSEEK_API_KEY"),
];

/// `probeEnvKeyName(prefix)` — the key name, or DeepSeek's for an unknown prefix.
///
/// **`hasOwnProperty` AGAIN, AND THIS TIME IT MATTERS IN BOTH DIRECTIONS.** The lookup is guarded, so
/// `"constructor"` gets the DEFAULT rather than `Object.prototype.constructor`'s source; and the default is
/// a real key rather than null, so an unknown channel probes the one channel that always has a key.
pub fn probe_env_key_name(prefix: &str) -> &'static str {
    PROBE_ENV_KEYS
        .iter()
        .find(|(k, _)| *k == prefix)
        .map(|(_, v)| *v)
        .unwrap_or("DEEPSEEK_API_KEY")
}

/// `encodeBase64Utf8(text)` — `btoa` over the UTF-8 bytes, with padding.
///
/// **THE JAVASCRIPT CHUNKS ITS `String.fromCharCode` CALL** (`0x8000` at a time) to keep a large body off
/// the argument stack, and the OUTPUT is what this has to match: standard base64 of the UTF-8 encoding,
/// padded. A `&str` here is already UTF-8, so the encoding step is the bytes.
pub fn encode_base64_utf8(text: &str) -> String {
    const TABLE: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let bytes = text.as_bytes();
    let mut out = String::with_capacity(bytes.len().div_ceil(3) * 4);
    for chunk in bytes.chunks(3) {
        let b = [
            chunk[0],
            *chunk.get(1).unwrap_or(&0),
            *chunk.get(2).unwrap_or(&0),
        ];
        let n = ((b[0] as u32) << 16) | ((b[1] as u32) << 8) | b[2] as u32;
        out.push(TABLE[(n >> 18) as usize & 63] as char);
        out.push(TABLE[(n >> 12) as usize & 63] as char);
        out.push(if chunk.len() > 1 {
            TABLE[(n >> 6) as usize & 63] as char
        } else {
            '='
        });
        out.push(if chunk.len() > 2 {
            TABLE[n as usize & 63] as char
        } else {
            '='
        });
    }
    out
}

#[cfg(test)]
mod oracle_corpus {
    //! `fixtures/translate-corpus.json`'s `tooling` cases were produced by the SHIPPING TypeScript
    //! (`gateway/wasm/oracle-translate.mjs`), and this replays them.
    use super::*;

    fn corpus() -> serde_json::Value {
        let path = concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/fixtures/translate-corpus.json"
        );
        let text = std::fs::read_to_string(path).expect("the corpus is committed");
        serde_json::from_str(&text).expect("the corpus parses")
    }

    #[test]
    fn every_tooling_helper_matches_the_shipping_typescript() {
        let doc = corpus();
        let mut checked = 0;
        for case in doc["cases"].as_array().expect("cases") {
            let func = case["fn"].as_str().unwrap_or("?");
            let name = case["name"].as_str().unwrap_or("?");
            let input = &case["input"];
            let want = &case["expected"]["value"];
            match func {
                "probe_env_key_name" => {
                    let got = probe_env_key_name(input.as_str().unwrap_or(""));
                    assert_eq!(
                        &serde_json::Value::String(got.to_string()),
                        want,
                        "{func} / {name}"
                    );
                }
                "encode_base64_utf8" => {
                    let got = encode_base64_utf8(input.as_str().unwrap_or(""));
                    assert_eq!(&serde_json::Value::String(got), want, "{func} / {name}");
                }
                _ => continue,
            }
            checked += 1;
        }
        assert!(
            checked >= 15,
            "the tooling corpus shrank to {checked} cases"
        );
    }
}
