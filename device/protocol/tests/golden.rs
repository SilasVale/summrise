//! The wire's golden fixtures: each shape parses, and serializes back to the same value.
//!
//! A fixture is the contract a console and a device build against. If a field is renamed, omitted by
//! default, or made `null`, one of these fails before either side ships a wire the other cannot read.

use serde_json::Value;
use summrise_protocol::{Answer, Body, Frame};

fn fixture(name: &str) -> Value {
    let text =
        std::fs::read_to_string(format!("tests/fixtures/{name}")).expect("the fixture reads");
    serde_json::from_str(&text).expect("the fixture is JSON")
}

fn round_trip<T>(value: &Value) -> Value
where
    T: for<'de> serde::Deserialize<'de> + serde::Serialize,
{
    let parsed: T = serde_json::from_value(value.clone()).expect("the fixture parses");
    serde_json::to_value(parsed).expect("the value serializes")
}

#[test]
fn every_frame_fixture_round_trips() {
    for name in [
        "call_token.json",
        "events.json",
        "follow.json",
        "unsubscribe.json",
    ] {
        let value = fixture(name);
        assert_eq!(round_trip::<Frame>(&value), value, "{name} round-trips");
    }
}

#[test]
fn every_answer_fixture_round_trips() {
    for name in ["answer_result.json", "answer_error.json"] {
        let value = fixture(name);
        assert_eq!(round_trip::<Answer>(&value), value, "{name} round-trips");
    }
}

#[test]
fn absent_members_are_omitted_not_null() {
    let frame = Frame::call(1, serde_json::json!({ "jsonrpc": "2.0" }));
    let value = serde_json::to_value(&frame).expect("serializes");
    assert_eq!(
        value,
        serde_json::json!({ "id": 1, "mcp": { "jsonrpc": "2.0" } })
    );

    let answer = Answer {
        id: 2,
        journal_seq: None,
        body: Body::Result {
            result: serde_json::json!({ "ok": true }),
        },
    };
    let value = serde_json::to_value(&answer).expect("serializes");
    assert_eq!(
        value,
        serde_json::json!({ "id": 2, "result": { "ok": true } })
    );
}

#[test]
fn the_constructors_build_the_documented_shapes() {
    assert_eq!(
        serde_json::to_value(Frame::events(9, 41)).expect("serializes"),
        fixture("events.json")
    );
    assert_eq!(
        serde_json::to_value(Frame::unsubscribe(12, 11)).expect("serializes"),
        fixture("unsubscribe.json")
    );

    let follow = Frame::follow(
        11,
        serde_json::json!({
            "jsonrpc": "2.0", "id": 11, "method": "tools/call",
            "params": { "name": "terminal.read", "arguments": { "id": "term-1", "cursor": 1050 } }
        }),
        "term-1".to_owned(),
        1050,
    );
    assert_eq!(
        serde_json::to_value(follow).expect("serializes"),
        fixture("follow.json")
    );
}

#[test]
fn an_answer_with_a_result_and_one_with_an_error_are_distinguishable() {
    let result: Answer = serde_json::from_value(fixture("answer_result.json")).expect("parses");
    assert!(matches!(result.body, Body::Result { .. }));
    let error: Answer = serde_json::from_value(fixture("answer_error.json")).expect("parses");
    assert!(matches!(error.body, Body::Error { .. }));
    assert_eq!(error.journal_seq, Some(42));
}
