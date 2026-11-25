//! A page of a view.
//!
//! Claims are ordered by `created`, then by id. `after` is the cursor of the
//! last claim on the previous page, so a verdict that changes behind the
//! cursor does not skip a later claim. A page judges the claims it returns,
//! plus one more when it needs to know that another page exists. It does not
//! judge the rest of the store.

use crate::claim::{self, Claim};
use crate::json::Value;
use crate::verdict::{self, Verdict};
use std::path::Path;

pub struct Query<'a> {
    pub limit: Option<usize>,
    pub offset: usize,
    pub filter: Option<Vec<&'a str>>,
    pub after: Option<(String, String)>,
    pub compact: bool,
    /// `id` selected one claim. `after` does not apply, and the count is that claim.
    pub scoped: bool,
}

pub fn parse_after(raw: Option<&str>) -> Result<Option<(String, String)>, String> {
    let Some(raw) = raw else {
        return Ok(None);
    };
    let Some((created, id)) = raw.split_once('/') else {
        return Err(
            "after must look like 2020-01-01T00:00:00Z/claim-id. Pass the next_after value from the previous page."
                .into(),
        );
    };
    if !crate::fresh::valid_created(created) || !crate::fsx::valid_slug(id) {
        return Err(
            "after must look like 2020-01-01T00:00:00Z/claim-id. Pass the next_after value from the previous page."
                .into(),
        );
    }
    Ok(Some((created.to_string(), id.to_string())))
}

pub fn assemble(
    project: &Path,
    claims: &[Claim],
    query: &Query<'_>,
    tool_args: &Hint<'_>,
) -> Value {
    let mut verified = 0u64;
    let mut refuted = 0u64;
    let mut unverified = 0u64;
    let mut rows = Vec::new();
    let mut skipped = 0usize;
    let mut last: Option<String> = None;
    let mut finished = true;
    let take = query.limit.unwrap_or(usize::MAX);
    for claim in claims {
        let viewed = verdict::view(project, claim);
        let keep = query
            .filter
            .as_ref()
            .is_none_or(|names| names.iter().any(|name| *name == viewed.verdict.name()));
        if keep && skipped >= query.offset && rows.len() >= take {
            // This claim proves another page exists. It is not part of the
            // page and it is not added to counts.
            finished = false;
            break;
        }
        match viewed.verdict {
            Verdict::Verified => verified += 1,
            Verdict::Refuted => refuted += 1,
            Verdict::Unverified => unverified += 1,
        }
        if !keep {
            continue;
        }
        if skipped < query.offset {
            skipped += 1;
            continue;
        }
        last = Some(claim::cursor(claim));
        rows.push(viewed.to_value(claim, query.compact));
    }
    let rows_listed = rows.len();
    let truncated = !finished;
    let counts_complete = finished && (query.scoped || query.after.is_none());
    let next_offset = if truncated && query.after.is_none() {
        Some(query.offset as u64 + rows.len() as u64)
    } else {
        None
    };
    let next_after = if truncated { last } else { None };
    let mut fields = vec![
        ("claims", Value::Array(rows)),
        (
            "counts",
            Value::obj(vec![
                ("verified", Value::uint(verified)),
                ("refuted", Value::uint(refuted)),
                ("unverified", Value::uint(unverified)),
            ]),
        ),
        ("counts_complete", Value::Bool(counts_complete)),
        ("truncated", Value::Bool(truncated)),
        (
            "next_offset",
            match next_offset {
                Some(n) => Value::uint(n),
                None => Value::Null,
            },
        ),
        (
            "next_after",
            match &next_after {
                Some(cursor) => Value::str(cursor),
                None => Value::Null,
            },
        ),
    ];
    if let Some(hint) = hint(tool_args, query.limit, next_after.as_deref(), rows_listed) {
        fields.push(("hint", Value::str(hint)));
    }
    Value::obj(fields)
}

pub struct Hint<'a> {
    pub id: Option<&'a str>,
    pub verdict: Option<&'a str>,
    pub states: Vec<&'a str>,
    pub compact: bool,
}

fn hint(
    args: &Hint<'_>,
    limit: Option<usize>,
    after: Option<&str>,
    listed: usize,
) -> Option<String> {
    if let Some(after) = after {
        let lim = limit.unwrap_or(100);
        return Some(format!("next: {}", call(args, lim, after)));
    }
    if limit.is_none() && listed > 100 {
        return Some(format!(
            "{listed} claims. Pass limit to page, then pass after as the next_after cursor. For example adsertio_view with {{\"limit\":100}}."
        ));
    }
    None
}

fn call(args: &Hint<'_>, limit: usize, after: &str) -> String {
    let mut fields = Vec::new();
    if let Some(id) = args.id {
        fields.push(format!("\"id\":\"{}\"", json_escape(id)));
    }
    if let Some(verdict) = args.verdict {
        fields.push(format!("\"verdict\":\"{}\"", json_escape(verdict)));
    }
    if !args.states.is_empty() {
        let items: Vec<String> = args
            .states
            .iter()
            .map(|state| format!("\"{}\"", json_escape(state)))
            .collect();
        fields.push(format!("\"states\":[{}]", items.join(",")));
    }
    if args.compact {
        fields.push("\"compact\":true".to_string());
    }
    fields.push(format!("\"limit\":{limit}"));
    fields.push(format!("\"after\":\"{}\"", json_escape(after)));
    format!("adsertio_view with {{{}}}", fields.join(","))
}

fn json_escape(text: &str) -> String {
    let mut out = String::new();
    for ch in text.chars() {
        match ch {
            '\\' => out.push_str("\\\\"),
            '"' => out.push_str("\\\""),
            c if c.is_control() => out.push(' '),
            c => out.push(c),
        }
    }
    out
}
