/// Pure self-register planning seam (boundary review 2026-09-06: the exact
/// "extract pure logic for testability" pattern — the network call lives in
/// main.rs's register loop, this fn is trivially unit-testable).
pub fn self_register_plan(
    console: Option<&str>,
    token: &str,
    hostname: &str,
    relay_device: Option<&str>,
) -> Option<(String, String)> {
    let console = console?.trim();
    if console.is_empty() || token.trim().is_empty() || hostname.trim().is_empty() {
        return None;
    }
    let name = hostname.split('.').next().unwrap_or("device").to_string();
    // **AND THE TRANSPORT THIS DEVICE IS REACHED BY (2026-10-17).** A device behind a relay is dialled BY NAME on
    // that relay (`gateway/wasm/src/device.rs`'s `device_fetch_via`), so the registration has to say WHICH NAME —
    // and it is **THE SAME VALUE THE POLL CARRIES** (`server.relay_device`, `agent/src/relay.rs`): a device that
    // polls as one name and registers another is reachable by exactly nobody, because the console would dial a
    // name the relay never sees.
    //
    // **AN ABSENT NAME DECLARES NOTHING**, which is the honest reading rather than a convenience: the agent then
    // polls the relay's DEFAULT device, and "the default device" is not something a per-device address can
    // express. One sentence follows for the operator — set `relay_device`, and give that name to the console —
    // and it is why an unnamed relay device stays tunnel-addressed instead of half-addressed.
    let mut body = serde_json::json!({ "name": &name, "hostname": hostname, "token": token });
    if let Some(relay) = relay_device.map(str::trim).filter(|d| !d.is_empty()) {
        body["relayDevice"] = serde_json::Value::String(relay.to_string());
    }
    let body = body.to_string();
    Some((
        format!(
            "{}/api/devices/self-register",
            console.trim_end_matches('/')
        ),
        body,
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    const HOST: &str = "d1.agent.saisi.online";
    const TOK: &str = "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa"; // 64 hex

    #[test]
    fn full_config_plans_url_and_body() {
        let (url, body) =
            self_register_plan(Some("https://api.saisi.online/"), TOK, HOST, None).unwrap();
        // Trailing slash on the console must not double up.
        assert_eq!(url, "https://api.saisi.online/api/devices/self-register");
        let v: serde_json::Value = serde_json::from_str(&body).unwrap();
        assert_eq!(v["name"], "d1", "name = first DNS label of the hostname");
        assert_eq!(v["hostname"], HOST);
        assert_eq!(v["token"], TOK);
    }

    #[test]
    fn pure_local_never_plans_a_send() {
        // The saisi decouple: no console_url → None (nothing is ever sent).
        assert_eq!(self_register_plan(None, TOK, HOST, None), None);
        assert_eq!(
            self_register_plan(Some("  "), TOK, HOST, None),
            None,
            "blank console"
        );
        assert_eq!(
            self_register_plan(Some("https://x"), "  ", HOST, None),
            None,
            "blank token"
        );
        assert_eq!(
            self_register_plan(Some("https://x"), TOK, "  ", None),
            None,
            "blank hostname"
        );
    }

    #[test]
    fn name_falls_back_and_url_tolerates_trailing_slashes() {
        // A hostname without dots still plans (name = whole label).
        let (url, body) = self_register_plan(Some("https://gw"), TOK, "barebox", None).unwrap();
        assert_eq!(url, "https://gw/api/devices/self-register");
        let v: serde_json::Value = serde_json::from_str(&body).unwrap();
        assert_eq!(v["name"], "barebox");
        // Multiple trailing slashes are trimmed once per trim_end_matches.
        let (url2, _) = self_register_plan(Some("https://gw///"), TOK, HOST, None).unwrap();
        assert_eq!(url2, "https://gw/api/devices/self-register");
    }

    #[test]
    fn a_named_relay_is_declared_and_an_unnamed_one_is_not() {
        let (_, body) =
            self_register_plan(Some("https://gw"), TOK, HOST, Some("workshop-pc")).unwrap();
        let v: serde_json::Value = serde_json::from_str(&body).unwrap();
        assert_eq!(v["relayDevice"], "workshop-pc");
        // The console's own name for the device is unchanged by the transport.
        assert_eq!(v["name"], "d1");

        // **NO RELAY, OR NO NAME, DECLARES NOTHING** — the record keeps the tunnel transport it has always had.
        for relay in [None, Some(""), Some("   ")] {
            let (_, body) = self_register_plan(Some("https://gw"), TOK, HOST, relay).unwrap();
            let v: serde_json::Value = serde_json::from_str(&body).unwrap();
            assert!(
                v.get("relayDevice").is_none(),
                "{relay:?} must declare no transport"
            );
        }
        // A name is trimmed before it is declared, because the poll's query is trimmed too.
        let (_, body) = self_register_plan(Some("https://gw"), TOK, HOST, Some(" d1 ")).unwrap();
        let v: serde_json::Value = serde_json::from_str(&body).unwrap();
        assert_eq!(v["relayDevice"], "d1");
    }
}
