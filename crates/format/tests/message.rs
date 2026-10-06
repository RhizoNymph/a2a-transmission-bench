//! Messages: ids are their bodies', part text follows the table, and
//! role/part pairs outside the spec's are unrepresentable.
#![allow(clippy::panic, clippy::unwrap_used, clippy::expect_used)]

mod common;

use a2a_bench_format::json::CanonicalJson;
use a2a_bench_format::message::{
    AssistantPart, Body, InvalidMessage, Message, NoPartText, ResultContent, ToolArguments,
    ToolCall, ToolExecution, ToolOutcome, ToolPart, ToolResult, UserPart,
};

fn result(content: Vec<ResultContent>) -> ToolResult {
    ToolResult {
        call_id: "c".into(),
        content,
        outcome: ToolOutcome::Success,
    }
}

#[test]
fn ids_are_stable_and_content_addressed() {
    let a = common::user("hello");
    let b = common::user("hello");
    let c = common::user("hello!");
    assert_eq!(a.id(), b.id());
    assert_ne!(a.id(), c.id());
    let json = serde_json::to_string(&a).unwrap();
    assert!(json.starts_with(r#"{"id":""#), "{json}");
    assert!(
        json.contains(r#""body":{"role":"user","parts":[{"type":"text","text":"hello"}]}"#),
        "{json}"
    );
    let back: Message = serde_json::from_str(&json).unwrap();
    assert_eq!(back, a);
}

#[test]
fn a_wrong_id_is_refused() {
    let json = serde_json::to_string(&common::user("hello")).unwrap();
    let other = common::user("other").id().to_string();
    let tampered = json.replacen(&json[7..71], &other, 1);
    let error = serde_json::from_str::<Message>(&tampered)
        .unwrap_err()
        .to_string();
    assert!(error.contains("is not its body's"), "{error}");
}

#[test]
fn unknown_fields_and_role_part_mismatches_are_refused() {
    let base = serde_json::to_value(common::user("x")).unwrap();
    let mut extra = base.clone();
    extra["extra"] = serde_json::json!(1);
    assert!(serde_json::from_value::<Message>(extra).is_err());
    let mut reasoning_in_user = base;
    reasoning_in_user["body"]["parts"] = serde_json::json!([{"type": "reasoning", "text": "hm"}]);
    assert!(serde_json::from_value::<Message>(reasoning_in_user).is_err());
}

#[test]
fn an_empty_tool_message_is_refused() {
    assert_eq!(
        Message::new(Body::Tool(vec![])),
        Err(InvalidMessage::EmptyTool)
    );
}

#[test]
fn part_text_follows_the_table() {
    let arguments = CanonicalJson::canonicalize(r#"{"b": 1, "a": "x"}"#).unwrap();
    let assistant = common::message(Body::Assistant(vec![
        AssistantPart::Text { text: "t".into() },
        AssistantPart::Reasoning { text: "r".into() },
        AssistantPart::ReasoningOpaque,
        AssistantPart::ToolCall(ToolCall {
            call_id: "call_sig__thought__QUJD".into(),
            name: "f".into(),
            arguments: ToolArguments::Json(arguments),
            execution: ToolExecution::Client,
        }),
        AssistantPart::ToolCall(ToolCall {
            call_id: "c2".into(),
            name: "f".into(),
            arguments: ToolArguments::Invalid("{not json".into()),
            execution: ToolExecution::Server,
        }),
        AssistantPart::ServerToolResult(result(vec![ResultContent::Text { text: "s".into() }])),
        AssistantPart::Unknown,
    ]));
    assert_eq!(assistant.part_text(0).unwrap(), "t");
    assert_eq!(assistant.part_text(1).unwrap(), "r");
    assert_eq!(
        assistant.part_text(2),
        Err(NoPartText::NotText { index: 2 })
    );
    assert_eq!(assistant.part_text(3).unwrap(), r#"{"a":"x","b":1}"#);
    assert_eq!(assistant.part_text(4).unwrap(), "{not json");
    assert_eq!(assistant.part_text(5).unwrap(), "s");
    assert_eq!(
        assistant.part_text(6),
        Err(NoPartText::NotText { index: 6 })
    );
    assert_eq!(
        assistant.part_text(7),
        Err(NoPartText::NoSuchPart { index: 7, parts: 7 })
    );
}

#[test]
fn tool_result_text_joins_text_contents_with_newlines() {
    let tool = common::message(Body::Tool(vec![
        ToolPart::ToolResult(result(vec![
            ResultContent::Text { text: "one".into() },
            ResultContent::Media {
                media_type: "image/png".into(),
            },
            ResultContent::Text { text: "two".into() },
        ])),
        ToolPart::ToolResult(result(vec![ResultContent::Media {
            media_type: "image/png".into(),
        }])),
    ]));
    assert_eq!(tool.part_text(0).unwrap(), "one\ntwo");
    assert_eq!(tool.part_text(1), Err(NoPartText::NotText { index: 1 }));
    let user = common::message(Body::User(vec![UserPart::Media {
        media_type: "image/png".into(),
    }]));
    assert_eq!(user.part_text(0), Err(NoPartText::NotText { index: 0 }));
}
