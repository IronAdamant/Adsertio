//! Every agent turn reads the tool definitions and the server instructions
//! again, so their size is a cost on every call. These budgets keep them from
//! growing back.
mod common;
use common::*;

/// The `tools` array of `tools/list`, as compact JSON.
const TOOLS_BUDGET: usize = 4_000;
/// The `instructions` string of `initialize`.
const INSTRUCTIONS_BUDGET: usize = 500;

/// The raw text of `key`'s value in a compact JSON reply: an array or a
/// string, found by matching brackets and quotes.
fn raw_value<'a>(reply: &'a str, key: &str) -> &'a str {
    let tag = format!("\"{key}\":");
    let start = reply.find(&tag).expect("key in reply") + tag.len();
    let bytes = reply.as_bytes();
    let (mut depth, mut in_str, mut escaped) = (0i32, false, false);
    for (i, &b) in bytes.iter().enumerate().skip(start) {
        if in_str {
            if escaped {
                escaped = false;
            } else if b == b'\\' {
                escaped = true;
            } else if b == b'"' {
                in_str = false;
                if depth == 0 {
                    return &reply[start..=i];
                }
            }
            continue;
        }
        match b {
            b'"' => in_str = true,
            b'[' | b'{' => depth += 1,
            b']' | b'}' => {
                depth -= 1;
                if depth == 0 {
                    return &reply[start..=i];
                }
            }
            _ => {}
        }
    }
    panic!("unterminated value for {key}");
}

#[test]
fn tool_definitions_and_instructions_stay_within_budget() {
    let dir = project("budget");
    init(&dir);
    let replies = talk(
        &dir,
        b"{\"jsonrpc\":\"2.0\",\"id\":1,\"method\":\"initialize\",\"params\":{\"protocolVersion\":\"2025-06-18\"}}\n{\"jsonrpc\":\"2.0\",\"id\":2,\"method\":\"tools/list\"}\n",
    );
    assert_eq!(replies.len(), 2, "{replies:?}");
    let instructions = raw_value(&replies[0], "instructions");
    // The quotes are not part of the text; nothing in it needs escaping.
    let text = &instructions[1..instructions.len() - 1];
    assert!(!text.contains('\\'), "{text}");
    assert!(
        text.len() < INSTRUCTIONS_BUDGET,
        "instructions are {} bytes, budget {INSTRUCTIONS_BUDGET}: {text}",
        text.len()
    );
    let tools = raw_value(&replies[1], "tools");
    assert!(tools.starts_with('['), "{tools}");
    assert!(
        tools.len() <= TOOLS_BUDGET,
        "tools/list tools array is {} bytes, budget {TOOLS_BUDGET}",
        tools.len()
    );
    for name in [
        "adsertio_claim",
        "adsertio_view",
        "adsertio_attach",
        "adsertio_ls",
        "adsertio_doctor",
        "adsertio_guide",
    ] {
        assert!(tools.contains(&format!("\"name\":\"{name}\"")), "{name}");
    }
    assert_eq!(tools.matches("\"name\":").count(), 6, "{tools}");
}
