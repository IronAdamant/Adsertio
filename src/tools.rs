//! The tool table: what an agent can do, over MCP or the CLI.

use crate::claim::{self, Claim};
use crate::fsx;
use crate::json::Value;
use crate::store::Store;
use crate::tool::{Args, Kind, Param, Tool};
use crate::verdict;

pub const GROUPS: &[&str] = &["core"];

pub static TOOLS: &[Tool] = &[
    Tool {
        name: "adsertio_claim",
        group: "core",
        description: "Record a claim (one line: something is true) with pointers to the evidence that would prove it. Evidence stays in Trammel, Chisel or Probatio; nothing is copied. The verdict is computed by adsertio_view, never stored.",
        params: &[
            Param { name: "summary", kind: Kind::Str, required: true, doc: "The claim, one line, up to 200 characters.", choices: &[] },
            Param { name: "evidence", kind: Kind::StrList, required: false, doc: "Pointers, each tool:path[#selector] with tool trammel, chisel or probatio and a path inside the project.", choices: &[] },
            Param { name: "author", kind: Kind::Str, required: false, doc: "Agent name slug. Default: agent.", choices: &[] },
            Param { name: "body", kind: Kind::Str, required: false, doc: "Why the claim matters, under about 300 words.", choices: &[] },
            Param { name: "id", kind: Kind::Str, required: false, doc: "Slug. Made from the summary if absent. Never overwrites.", choices: &[] },
            Param { name: "tags", kind: Kind::StrList, required: false, doc: "Tag slugs.", choices: &[] },
        ],
        run: claim_add,
    },
    Tool {
        name: "adsertio_view",
        group: "core",
        description: "The verified or unverified view: each claim with its verdict and the state of every piece of evidence. Verified needs evidence and all of it passing; refuted if any fails; otherwise unverified, with the reason per pointer.",
        params: &[
            Param { name: "id", kind: Kind::Str, required: false, doc: "Only this claim.", choices: &[] },
            Param { name: "verdict", kind: Kind::Str, required: false, doc: "Only claims with this verdict. Do not also pass states.", choices: &["verified", "refuted", "unverified"] },
            Param { name: "states", kind: Kind::StrList, required: false, doc: "Only claims with one of these verdicts: verified, refuted, unverified. Do not also pass verdict.", choices: &[] },
            Param { name: "compact", kind: Kind::Bool, required: false, doc: "Omit evidence detail. Each pointer still has its state.", choices: &[] },
            Param { name: "limit", kind: Kind::UInt, required: false, doc: "How many claims to return. Omit it to return every matching claim. Maximum 500 when paging.", choices: &[] },
            Param { name: "offset", kind: Kind::UInt, required: false, doc: "Skip this many matching claims. Omit it, and limit, to return every claim. Prefer after: offset can skip a claim when a verdict changes between pages.", choices: &[] },
            Param { name: "after", kind: Kind::Str, required: false, doc: "Stable cursor. Pass next_after from the previous page. Claims are ordered by created, then by id.", choices: &[] },
        ],
        run: view,
    },
    Tool {
        name: "adsertio_attach",
        group: "core",
        description: "Add evidence pointers to an existing claim. The file is replaced whole. A bad pointer or a duplicate is rejected and the claim is left unchanged.",
        params: &[
            Param { name: "id", kind: Kind::Str, required: true, doc: "Claim id.", choices: &[] },
            Param { name: "evidence", kind: Kind::StrList, required: true, doc: "Pointers to add, each tool:path[#selector]. Duplicates are an error.", choices: &[] },
        ],
        run: attach,
    },
    Tool {
        name: "adsertio_ls",
        group: "core",
        description: "List claim ids, summaries, and how many pointers each has. Does not read the evidence and does not report a verdict; call adsertio_view for that. Returns every claim unless limit or offset is set. count is the whole store.",
        params: &[
            Param { name: "limit", kind: Kind::UInt, required: false, doc: "How many claims to return. Omit it to return every claim. Maximum 500 when paging.", choices: &[] },
            Param { name: "offset", kind: Kind::UInt, required: false, doc: "Skip this many claims. Omit it, and limit, to return every claim.", choices: &[] },
        ],
        run: ls,
    },
    Tool {
        name: "adsertio_check",
        group: "core",
        description: "Read every claim once. Fail on damage, a bad file name, a bad created time, or a malformed evidence timestamp. A lock or temp file whose writer is gone is removed. Same as the adsertio check command.",
        params: &[],
        run: check_tool,
    },
    Tool {
        name: "adsertio_guide",
        group: "core",
        description: "Short guide for an agent: how a verdict is decided, the pointer shapes, paging, and what to call after the conversation is compacted. Does not read the store.",
        params: &[],
        run: guide,
    },
];

fn claim_add(a: &Args) -> Result<Value, String> {
    let store = Store::open()?;
    crate::store::claims_ready(&store, false)?;
    let summary = a.str("summary").unwrap_or_default();
    claim::validate_summary(summary)?;
    let mut evidence: Vec<String> = a.list("evidence").iter().map(|s| s.to_string()).collect();
    for p in &evidence {
        claim::valid_pointer(p)?;
    }
    evidence = dedupe(evidence);
    if evidence.len() > claim::MAX_POINTERS {
        return Err(format!(
            "a claim holds {} pointers at most",
            claim::MAX_POINTERS
        ));
    }
    let mut tags: Vec<String> = a.list("tags").iter().map(|s| s.to_string()).collect();
    if let Some(bad) = tags.iter().find(|t| !fsx::valid_slug(t)) {
        return Err(format!("bad tag \"{}\": use a slug", fsx::clip(bad, 40)));
    }
    tags = dedupe(tags);
    let author = a.str("author").unwrap_or("agent");
    if !fsx::valid_slug(author) {
        return Err(format!(
            "invalid author \"{}\": use a slug",
            fsx::clip(author, 40)
        ));
    }
    let body = a.str("body").unwrap_or_default();
    if body.len() > claim::MAX_BODY_BYTES {
        return Err(format!(
            "body is longer than {} bytes",
            claim::MAX_BODY_BYTES
        ));
    }
    let explicit = a.str("id").map(str::to_string);
    let slug = explicit.clone().unwrap_or_else(|| slug_from(summary));
    let dir = store.claims_dir();
    for n in 1..1000 {
        if explicit.is_some() && n > 1 {
            break;
        }
        let id = match &explicit {
            Some(id) => id.clone(),
            None if n == 1 => slug.clone(),
            None => format!("{slug}-{n}"),
        };
        let path = fsx::join_id(&dir, &id, 1)?.with_extension("md");
        let c = Claim {
            id: id.clone(),
            summary: summary.to_string(),
            author: author.to_string(),
            created: fsx::now_iso(),
            evidence: evidence.clone(),
            tags: tags.clone(),
            body: body.to_string(),
        };
        let bytes = c.render().into_bytes();
        if bytes.len() as u64 > claim::MAX_CLAIM_BYTES {
            return Err(format!(
                "claim \"{id}\" would be larger than {} bytes",
                claim::MAX_CLAIM_BYTES
            ));
        }
        match fsx::write_atomic(&path, &bytes, false) {
            Ok(()) => {
                return Ok(Value::obj(vec![
                    ("id", Value::String(id)),
                    ("evidence", Value::uint(c.evidence.len() as u64)),
                ]));
            }
            Err(e) if e.contains("already exists") && explicit.is_none() => continue,
            Err(e) if e.contains("already exists") => {
                return Err(format!(
                    "claim \"{id}\" already exists at .adsertio/claims/{id}.md. Use another id, or adsertio_attach to add evidence."
                ));
            }
            Err(e) => return Err(format!("claim \"{id}\" not written: {e}")),
        }
    }
    Err("could not find a free id; pass id".into())
}

fn dedupe(items: Vec<String>) -> Vec<String> {
    let mut kept = Vec::new();
    for item in items {
        if !kept.iter().any(|seen: &String| seen == &item) {
            kept.push(item);
        }
    }
    kept
}

fn slug_from(summary: &str) -> String {
    let mut slug: String = summary
        .to_lowercase()
        .chars()
        .map(|c| if c.is_ascii_alphanumeric() { c } else { '-' })
        .collect::<String>()
        .split('-')
        .filter(|s| !s.is_empty())
        .collect::<Vec<_>>()
        .join("-");
    slug.truncate(48);
    let slug = slug.trim_matches('-').to_string();
    if slug.is_empty() {
        "claim".to_string()
    } else {
        slug
    }
}

fn attach(a: &Args) -> Result<Value, String> {
    let store = Store::open()?;
    crate::store::claims_ready(&store, false)?;
    let id = a.str("id").unwrap_or("");
    let incoming: Vec<String> = a
        .list("evidence")
        .iter()
        .map(|s| (*s).to_string())
        .collect();
    if incoming.is_empty() {
        return Err("evidence is empty; pass one or more pointers".into());
    }
    for pointer in &incoming {
        claim::valid_pointer(pointer)?;
    }
    let mut seen = Vec::new();
    for pointer in &incoming {
        if seen.iter().any(|kept| kept == pointer) {
            return Err(format!(
                "evidence \"{}\" is repeated",
                fsx::clip(pointer, 60)
            ));
        }
        seen.push(pointer.clone());
    }
    let path = fsx::join_id(&store.claims_dir(), id, 1)?.with_extension("md");
    let lock = store.claims_dir().join(format!(".{id}.md.lock"));
    fsx::with_claim_lock(&lock, || {
        let meta = match std::fs::symlink_metadata(&path) {
            Ok(meta) => meta,
            Err(_) => {
                return Err(format!(
                    "no claim with id \"{}\". Call adsertio_ls for the ids, or adsertio_claim to record one.",
                    fsx::clip(id, 60)
                ))
            }
        };
        if meta.file_type().is_symlink() || !meta.is_file() {
            return Err(format!("{} is not a claim file", path.display()));
        }
        let text = fsx::read_strict(&path)?;
        let mut claim = Claim::parse(id, &text)
            .map_err(|e| format!("corrupt claim {id} ({}): {e}", path.display()))?;
        for pointer in &incoming {
            if claim.evidence.iter().any(|kept| kept == pointer) {
                return Err(format!(
                    "evidence \"{}\" is already on claim \"{id}\"",
                    fsx::clip(pointer, 60)
                ));
            }
        }
        if claim.evidence.len() + incoming.len() > claim::MAX_POINTERS {
            return Err(format!(
                "claim \"{id}\" would have more than {} pointers. A claim holds {} at most; nothing was written.",
                claim::MAX_POINTERS,
                claim::MAX_POINTERS
            ));
        }
        claim.evidence.extend(incoming.iter().cloned());
        let bytes = claim.render().into_bytes();
        if bytes.len() as u64 > claim::MAX_CLAIM_BYTES {
            return Err(format!(
                "claim \"{id}\" would be larger than {} bytes; nothing was written.",
                claim::MAX_CLAIM_BYTES
            ));
        }
        fsx::write_atomic(&path, &bytes, true)
            .map_err(|e| format!("claim \"{id}\" not updated: {e}"))?;
        Ok(Value::obj(vec![
            ("id", Value::str(&claim.id)),
            ("evidence", Value::uint(claim.evidence.len() as u64)),
            (
                "added",
                Value::Array(incoming.iter().map(|p| Value::str(p.as_str())).collect()),
            ),
        ]))
    })
}

fn ls(a: &Args) -> Result<Value, String> {
    let (limit, offset) = page_args(a)?;
    let store = Store::open()?;
    let claims = claim::load_all(&store)?;
    let rows: Vec<Value> = claims
        .iter()
        .map(|claim| {
            Value::obj(vec![
                ("id", Value::str(&claim.id)),
                ("summary", Value::str(&claim.summary)),
                ("pointers", Value::uint(claim.evidence.len() as u64)),
            ])
        })
        .collect();
    let total = rows.len() as u64;
    Ok(finish_page(
        "adsertio_ls",
        rows,
        limit,
        offset,
        vec![("count", Value::uint(total))],
        a,
    ))
}

fn view(a: &Args) -> Result<Value, String> {
    let (limit, offset) = page_args(a)?;
    let filter = verdict_filter(a)?;
    let after = crate::paging::parse_after(a.str("after"))?;
    let compact = a.bool("compact") == Some(true);
    let store = Store::open()?;
    let mut claims = claim::load_all(&store)?;
    let scoped = a.str("id").is_some();
    if let Some(id) = a.str("id") {
        claims.retain(|c| c.id == id);
        if claims.is_empty() {
            return Err(format!(
                "no claim with id \"{}\". Call adsertio_ls for the ids, or adsertio_claim to record one.",
                fsx::clip(id, 60)
            ));
        }
    } else if let Some((created, id)) = &after {
        claims.retain(|claim| claim::is_after(claim, created, id));
    }
    let project = store.project.clone();
    let hint = crate::paging::Hint {
        id: a.str("id"),
        verdict: a.str("verdict"),
        states: a.list("states"),
        compact,
    };
    let query = crate::paging::Query {
        limit,
        offset,
        filter,
        after,
        compact,
        scoped,
    };
    crate::cache::with_cache(|| Ok(crate::paging::assemble(&project, &claims, &query, &hint)))
}

const MAX_PAGE: u64 = 500;
const LARGE_LIST: u64 = 100;

/// `None` means no verdict filter. `verdict` and `states` are not combined.
fn verdict_filter(a: &Args) -> Result<Option<Vec<&str>>, String> {
    let states = a.list("states");
    if a.str("verdict").is_some() && !states.is_empty() {
        return Err(
            "pass verdict or states, not both. states takes a list, for example [\"verified\", \"refuted\"]."
                .into(),
        );
    }
    for state in &states {
        if !matches!(*state, "verified" | "refuted" | "unverified") {
            return Err(format!(
                "states must be verified, refuted, or unverified, not \"{state}\""
            ));
        }
    }
    if let Some(verdict) = a.str("verdict") {
        return Ok(Some(vec![verdict]));
    }
    if states.is_empty() {
        Ok(None)
    } else {
        Ok(Some(states))
    }
}

fn page_args(a: &Args) -> Result<(Option<usize>, usize), String> {
    let offset = a.uint("offset").unwrap_or(0) as usize;
    let limit = match a.uint("limit") {
        None => None,
        Some(0) => return Err(crate::tool::LIMIT_RANGE.into()),
        Some(n) if n > MAX_PAGE => return Err(crate::tool::LIMIT_RANGE.into()),
        Some(n) => Some(n as usize),
    };
    Ok((limit, offset))
}

fn page(items: Vec<Value>, limit: Option<usize>, offset: usize) -> (Vec<Value>, bool, Option<u64>) {
    let take = limit.unwrap_or(usize::MAX);
    if offset >= items.len() {
        return (Vec::new(), false, None);
    }
    let end = offset.saturating_add(take).min(items.len());
    let truncated = end < items.len();
    let next = if truncated { Some(end as u64) } else { None };
    let rows = items.into_iter().skip(offset).take(take).collect();
    (rows, truncated, next)
}

fn finish_page(
    tool: &str,
    items: Vec<Value>,
    limit: Option<usize>,
    offset: usize,
    mut extra: Vec<(&str, Value)>,
    args: &Args,
) -> Value {
    let listed = items.len() as u64;
    let (rows, truncated, next) = page(items, limit, offset);
    extra.insert(0, ("claims", Value::Array(rows)));
    extra.push(("truncated", Value::Bool(truncated)));
    extra.push((
        "next_offset",
        match next {
            Some(n) => Value::uint(n),
            None => Value::Null,
        },
    ));
    if let Some(hint) = page_hint(tool, args, limit, next, listed) {
        extra.push(("hint", Value::str(hint)));
    }
    Value::obj(extra)
}

fn page_hint(
    tool: &str,
    args: &Args,
    limit: Option<usize>,
    next: Option<u64>,
    listed: u64,
) -> Option<String> {
    let (offset, lim) = if let Some(offset) = next {
        (offset, limit.unwrap_or(LARGE_LIST as usize))
    } else if limit.is_none() && listed > LARGE_LIST {
        (0, LARGE_LIST as usize)
    } else {
        return None;
    };
    let call = hint_call(tool, args, lim, offset);
    if next.is_some() {
        Some(format!("next: {call}"))
    } else {
        Some(format!(
            "{listed} claims. Pass limit and offset to page, for example {call}."
        ))
    }
}

fn hint_call(tool: &str, args: &Args, limit: usize, offset: u64) -> String {
    let mut fields = Vec::new();
    if let Some(id) = args.str("id") {
        fields.push(format!("\"id\":\"{}\"", json_escape(id)));
    }
    if let Some(verdict) = args.str("verdict") {
        fields.push(format!("\"verdict\":\"{}\"", json_escape(verdict)));
    }
    let states = args.list("states");
    if !states.is_empty() {
        let items: Vec<String> = states
            .iter()
            .map(|state| format!("\"{}\"", json_escape(state)))
            .collect();
        fields.push(format!("\"states\":[{}]", items.join(",")));
    }
    if args.bool("compact") == Some(true) {
        fields.push("\"compact\":true".to_string());
    }
    fields.push(format!("\"limit\":{limit}"));
    fields.push(format!("\"offset\":{offset}"));
    format!("{tool} with {{{}}}", fields.join(","))
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

fn check_tool(_a: &Args) -> Result<Value, String> {
    store_check()
}

pub fn store_check() -> Result<Value, String> {
    let store = Store::open()?;
    crate::store::claims_ready(&store, false)?;
    let mut problems = Vec::new();
    let mut strays = Vec::new();
    let dir = store.claims_dir();
    fsx::sweep_abandoned_temps(&dir);
    if let Ok(rd) = std::fs::read_dir(&dir) {
        for entry in rd.flatten() {
            let name = entry.file_name().to_string_lossy().to_string();
            let path = entry.path();
            if name.starts_with('.') && name.contains(".tmp-") {
                strays.push(Value::String(name));
                continue;
            }
            if name.starts_with('.') && name.ends_with(".md.lock") {
                fsx::break_abandoned_lock(&path);
                continue;
            }
            if name.starts_with('.') {
                continue;
            }
            let slug = name.strip_suffix(".md");
            let file = entry
                .file_type()
                .map(|kind| kind.is_file())
                .unwrap_or(false);
            if !file || slug.is_none_or(|id| !fsx::valid_slug(id)) {
                problems.push(format!(
                    ".adsertio/claims/{name} is not a claim file. Use a slug id ending in .md. It is left out of adsertio_ls."
                ));
            }
        }
    }
    let claims = claim::load_all(&store)?;
    for claim in &claims {
        if !crate::fresh::valid_created(&claim.created) {
            problems.push(format!(
                "claim \"{}\" created time \"{}\" must be YYYY-MM-DDTHH:MM:SSZ",
                claim.id, claim.created
            ));
        }
    }
    let project = store.project.clone();
    crate::cache::with_cache(|| {
        for claim in &claims {
            for pointer in &claim.evidence {
                let evidence = verdict::read_evidence(&project, pointer);
                if evidence.malformed {
                    problems.push(format!(
                        "claim \"{}\" pointer {} has a malformed timestamp or commit",
                        claim.id, evidence.pointer
                    ));
                }
            }
        }
    });
    if !problems.is_empty() {
        return Err(problems.join("\n"));
    }
    Ok(Value::obj(vec![
        ("ok", Value::Bool(true)),
        ("claims", Value::uint(claims.len() as u64)),
        ("stray_temp_files", Value::Array(strays)),
    ]))
}

fn guide(_a: &Args) -> Result<Value, String> {
    Ok(Value::obj(vec![
        (
            "verdict",
            Value::str(
                "verified: every pointer passed, and there is at least one. refuted: any pointer failed. unverified: anything else. Computed by adsertio_view, never stored.",
            ),
        ),
        (
            "after_compaction",
            Value::str(
                "Call adsertio_ls, then adsertio_view for the ids that matter. Do not repeat a verdict from memory.",
            ),
        ),
        (
            "selectors",
            Value::Array(vec![
                Value::str("trammel:.trammel/store#plan-<id>/step-<id>"),
                Value::str("trammel:.trammel/store#plan-<id>"),
                Value::str("trammel:.trammel/store#constraint-<id>"),
                Value::str("trammel:.trammel/store#failure-<id>"),
                Value::str("chisel:.chisel/store#test:<test_id>"),
                Value::str("chisel:.chisel/store#file:<path>"),
                Value::str("probatio:<out-dir>"),
                Value::str("probatio:<out-dir>#survivors"),
                Value::str("probatio:out/run#survivors"),
                Value::str("probatio:<out-dir>#mutant:<id>"),
                Value::str("probatio:<out-dir>#ledger"),
            ]),
        ),
        (
            "paging",
            Value::str(
                "Omit limit and adsertio_ls and adsertio_view return every matching claim. Pass limit (1 to 500) to page. For adsertio_view, pass after as the next_after cursor so a verdict that changes behind the cursor does not skip a claim. offset still skips a count of matches and can skip one when a verdict changes. counts on a page covers the claims that page judged; counts_complete is true when that is the whole scope. count on adsertio_ls is still the whole store.",
            ),
        ),
        (
            "writes",
            Value::str(
                "adsertio_claim never overwrites. Add pointers with adsertio_attach. A repeated pointer is an error and the file is left unchanged. A claim holds at most 50 pointers. The body is at most 20000 bytes. The claim file is at most 262144 bytes. Duplicate tags and evidence on a new claim are kept once.",
            ),
        ),
    ]))
}
