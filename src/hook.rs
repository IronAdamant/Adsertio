//! `adsertio hook --host claude|cursor|generic`: print a host hook that shows
//! the agent its open claims (unverified and refuted) at session start and
//! after compaction, so it reads the verdicts again instead of repeating them
//! from memory. The snippet is text for a person to paste. Adsertio does not
//! install it and does not run a background process.
//!
//! The command has no shell syntax (no `&&`, `||`, quotes, `~` or
//! variables), so it runs the same under cmd, PowerShell, Git Bash and sh.
//! The Claude Code snippet uses the exec form (`command` plus `args`), which
//! starts the program directly with no shell at all.

use crate::json::Value;
use crate::tool::{Kind, Param};

const PARAMS: &[Param] = &[Param {
    name: "host",
    kind: Kind::Str,
    required: false,
    doc: "claude, cursor or generic.",
    choices: &[],
}];

/// The view the hook runs: open claims only, without per-pointer detail, and
/// at most 50 of them so a big store cannot flood the session (the reply's
/// hint names the next page).
pub const VIEW_ARGS: &[&str] = &[
    "view",
    "--compact",
    "--states",
    "unverified",
    "--states",
    "refuted",
    "--limit",
    "50",
];

/// The `--wrap` values `adsertio view` takes, for hosts that want JSON.
pub const WRAPS: &[&str] = &["cursor", "claude-subagent"];

pub fn from_argv(argv: &[String]) -> Result<String, String> {
    let parsed = crate::cli::parse_flags(PARAMS, "hook", argv)
        .map_err(|e| format!("{e} (hook takes --host claude, cursor or generic)"))?;
    let host = parsed
        .get("host")
        .and_then(Value::as_str)
        .unwrap_or("generic")
        .to_string();
    snippet(&host)
}

fn command_line(extra: &[&str]) -> String {
    let mut words = vec!["adsertio"];
    words.extend_from_slice(VIEW_ARGS);
    words.extend_from_slice(extra);
    words.join(" ")
}

fn exec_entry(matcher: &str, extra: &[&str], indent: &str) -> String {
    let mut args: Vec<Value> = VIEW_ARGS.iter().map(|a| Value::str(*a)).collect();
    args.extend(extra.iter().map(|a| Value::str(*a)));
    let args = Value::Array(args).to_json();
    format!(
        "{indent}{{\n{indent}  \"matcher\": \"{matcher}\",\n{indent}  \"hooks\": [\n{indent}    {{\n{indent}      \"type\": \"command\",\n{indent}      \"command\": \"adsertio\",\n{indent}      \"args\": {args}\n{indent}    }}\n{indent}  ]\n{indent}}}"
    )
}

/// The JSON for `.claude/settings.json`, alone, so a test can parse it.
pub fn claude_json() -> String {
    format!(
        "{{\n  \"hooks\": {{\n    \"SessionStart\": [\n{}\n    ],\n    \"SubagentStart\": [\n{}\n    ]\n  }}\n}}\n",
        exec_entry("startup|resume|clear|compact", &[], "      "),
        exec_entry("*", &["--wrap", "claude-subagent"], "      "),
    )
}

/// The JSON for `.cursor/hooks.json`, alone.
pub fn cursor_json() -> String {
    format!(
        "{{\n  \"version\": 1,\n  \"hooks\": {{\n    \"sessionStart\": [\n      {{ \"command\": \"{}\" }}\n    ]\n  }}\n}}\n",
        command_line(&["--wrap", "cursor"])
    )
}

fn snippet(host: &str) -> Result<String, String> {
    let plain = command_line(&[]);
    let text = match host {
        "generic" => format!(
            "# Run this at session start and after compaction, and put its output into the session.\n\
             # It lists the claims that are not verified, with the state of each pointer.\n\
             # adsertio must be on PATH (on Windows, the folder holding adsertio.exe).\n\
             {plain}\n"
        ),
        "claude" => format!(
            "# Paste the JSON into .claude/settings.json (merge it with any hooks already there).\n\
             # SessionStart runs at startup, resume, clear and after compaction, and the\n\
             # command's output is added to the session, so the agent sees which claims are\n\
             # not verified instead of repeating verdicts from memory. SubagentStart gives a\n\
             # sub-agent the same list. The exec form (command plus args) starts adsertio\n\
             # with no shell, so it works the same on Windows, macOS and Linux.\n\
             # adsertio must be on PATH (on Windows, the folder holding adsertio.exe).\n\
             {}",
            claude_json()
        ),
        "cursor" => format!(
            "# Paste the JSON into .cursor/hooks.json.\n\
             # sessionStart runs when a chat starts. With --wrap cursor the command prints\n\
             # {{\"additional_context\": ...}} and Cursor adds that text to the session.\n\
             # preCompact cannot put text back into the model. After compaction, run\n\
             # `{plain}` again (or call adsertio_view with states\n\
             # [\"unverified\", \"refuted\"]) and put the output into the session yourself.\n\
             # adsertio must be on PATH (on Windows, the folder holding adsertio.exe).\n\
             {}",
            cursor_json()
        ),
        other => {
            return Err(format!(
                "unknown host \"{other}\" (use claude, cursor or generic)"
            ))
        }
    };
    Ok(text)
}

/// Wrap a view's JSON for a host that reads hook output as JSON.
pub fn wrap(host: &str, text: &str) -> Result<String, String> {
    let context = format!(
        "Adsertio claims that are not verified (unverified or refuted). Read verdicts with adsertio_view; do not repeat one from memory.\n{text}"
    );
    let value = match host {
        "cursor" => Value::obj(vec![("additional_context", Value::String(context))]),
        "claude-subagent" => Value::obj(vec![(
            "hookSpecificOutput",
            Value::obj(vec![
                ("hookEventName", Value::str("SubagentStart")),
                ("additionalContext", Value::String(context)),
            ]),
        )]),
        other => {
            return Err(format!(
                "--wrap must be one of: {}, not \"{other}\"",
                WRAPS.join(", ")
            ))
        }
    };
    Ok(value.to_json())
}
