//! The tool table: what an agent can do, over MCP or the CLI.

use crate::claim::{self, Claim};
use crate::fsx;
use crate::json::Value;
use crate::store::Store;
use crate::tool::{Args, Kind, Param, Tool, MAX_PAGE};
use crate::verdict;

pub const GROUPS: &[&str] = &["core"];

pub static TOOLS: &[Tool] = &[
    Tool {
        name: "adsertio_claim",
        group: "core",
        description: "Record claims (one line each: something is true) with evidence pointers tool:path[#selector] (tool trammel, chisel or probatio; path inside the project). Record all claims in one call with claims; a bad item writes nothing. Never overwrites.",
        params: &[
            Param { name: "claims", kind: Kind::ObjList, required: false, doc: "Several claims, each with the fields below. Not with the single-claim fields.", choices: &[], fields: CLAIM_FIELDS },
            Param { name: "summary", kind: Kind::Str, required: false, doc: "One claim: one line, at most 200 characters.", choices: &[], fields: &[] },
            Param { name: "evidence", kind: Kind::StrList, required: false, doc: "", choices: &[], fields: &[] },
            Param { name: "author", kind: Kind::Str, required: false, doc: "Agent slug. Default: agent.", choices: &[], fields: &[] },
            Param { name: "body", kind: Kind::Str, required: false, doc: "Why it matters, under 300 words.", choices: &[], fields: &[] },
            Param { name: "id", kind: Kind::Str, required: false, doc: "Slug. Default: made from summary.", choices: &[], fields: &[] },
            Param { name: "tags", kind: Kind::StrList, required: false, doc: "Slugs.", choices: &[], fields: &[] },
        ],
        run: claim_add,
    },
    Tool {
        name: "adsertio_view",
        group: "core",
        description: "Claims with their verdict and each pointer's state. verified: has evidence, all passing; refuted: any failing; else unverified, with a reason per pointer. Omit limit for every match; if truncated, pass next_after as after.",
        params: &[
            Param { name: "id", kind: Kind::Str, required: false, doc: "Only this claim.", choices: &[], fields: &[] },
            Param { name: "verdict", kind: Kind::Str, required: false, doc: "Not with states.", choices: &["verified", "refuted", "unverified"], fields: &[] },
            Param { name: "states", kind: Kind::StrList, required: false, doc: "Any of these verdicts. Not with verdict.", choices: &["verified", "refuted", "unverified"], fields: &[] },
            Param { name: "compact", kind: Kind::Bool, required: false, doc: "Pointer states without detail.", choices: &[], fields: &[] },
            Param { name: "limit", kind: Kind::UInt, required: false, doc: "", choices: &[], fields: &[] },
            Param { name: "offset", kind: Kind::UInt, required: false, doc: "Skip this many matches. Prefer after.", choices: &[], fields: &[] },
            Param { name: "after", kind: Kind::Str, required: false, doc: "Cursor: next_after from the last page.", choices: &[], fields: &[] },
        ],
        run: view,
    },
    Tool {
        name: "adsertio_attach",
        group: "core",
        description: "Add evidence pointers to existing claims. For several claims use one call with items; a bad, repeated or already present pointer, or an unknown id, writes nothing.",
        params: &[
            Param { name: "items", kind: Kind::ObjList, required: false, doc: "One {id, evidence} per claim. Not with id or evidence.", choices: &[], fields: ATTACH_FIELDS },
            Param { name: "id", kind: Kind::Str, required: false, doc: "One claim id.", choices: &[], fields: &[] },
            Param { name: "evidence", kind: Kind::StrList, required: false, doc: "Pointers to add to it.", choices: &[], fields: &[] },
        ],
        run: attach,
    },
    Tool {
        name: "adsertio_ls",
        group: "core",
        description: "Claim ids, summaries and pointer counts. Cheap: reads no evidence and gives no verdict. count is the whole store.",
        params: &[
            Param { name: "limit", kind: Kind::UInt, required: false, doc: "", choices: &[], fields: &[] },
            Param { name: "offset", kind: Kind::UInt, required: false, doc: "Skip this many.", choices: &[], fields: &[] },
        ],
        run: ls,
    },
    Tool {
        name: "adsertio_doctor",
        group: "core",
        description: "Find what is wrong: the store, damaged claims, malformed evidence times, git, and the Trammel, Chisel and Probatio files. Returns {ok, checks}; each failing check has a fix.",
        params: &[
            Param { name: "fix", kind: Kind::Bool, required: false, doc: "First remove temp and lock files left by a dead writer.", choices: &[], fields: &[] },
        ],
        run: doctor,
    },
    Tool {
        name: "adsertio_guide",
        group: "core",
        description: "How a verdict is decided, pointer shapes, paging, and what to do after compaction.",
        params: &[],
        run: guide,
    },
];

/// The fields of one claim: the single-claim arguments, and each object in
/// `claims`.
const CLAIM_FIELDS: &[Param] = &[
    Param {
        name: "summary",
        kind: Kind::Str,
        required: true,
        doc: "The claim, one line, up to 200 characters.",
        choices: &[],
        fields: &[],
    },
    Param {
        name: "evidence",
        kind: Kind::StrList,
        required: false,
        doc: "Pointers, each tool:path[#selector].",
        choices: &[],
        fields: &[],
    },
    Param {
        name: "author",
        kind: Kind::Str,
        required: false,
        doc: "Agent name slug. Default: agent.",
        choices: &[],
        fields: &[],
    },
    Param {
        name: "body",
        kind: Kind::Str,
        required: false,
        doc: "Why the claim matters.",
        choices: &[],
        fields: &[],
    },
    Param {
        name: "id",
        kind: Kind::Str,
        required: false,
        doc: "Slug. Made from the summary if absent.",
        choices: &[],
        fields: &[],
    },
    Param {
        name: "tags",
        kind: Kind::StrList,
        required: false,
        doc: "Tag slugs.",
        choices: &[],
        fields: &[],
    },
];

/// The fields of one attach: the single-attach arguments, and each object in
/// `items`.
const ATTACH_FIELDS: &[Param] = &[
    Param {
        name: "id",
        kind: Kind::Str,
        required: true,
        doc: "Claim id.",
        choices: &[],
        fields: &[],
    },
    Param {
        name: "evidence",
        kind: Kind::StrList,
        required: true,
        doc: "Pointers to add.",
        choices: &[],
        fields: &[],
    },
];

/// A claim that passed every check and is ready to write.
struct Draft {
    explicit: Option<String>,
    slug: String,
    summary: String,
    author: String,
    body: String,
    evidence: Vec<String>,
    tags: Vec<String>,
}

fn draft(a: &Args) -> Result<Draft, String> {
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
    if let Some(id) = &explicit {
        fsx::join_id(std::path::Path::new("."), id, 1)?;
    }
    let slug = explicit.clone().unwrap_or_else(|| slug_from(summary));
    Ok(Draft {
        explicit,
        slug,
        summary: summary.to_string(),
        author: author.to_string(),
        body: body.to_string(),
        evidence,
        tags,
    })
}

impl Draft {
    fn claim(&self, id: &str) -> Result<(Claim, Vec<u8>), String> {
        let c = Claim {
            id: id.to_string(),
            summary: self.summary.clone(),
            author: self.author.clone(),
            created: fsx::now_iso(),
            evidence: self.evidence.clone(),
            tags: self.tags.clone(),
            body: self.body.clone(),
        };
        let bytes = c.render().into_bytes();
        if bytes.len() as u64 > claim::MAX_CLAIM_BYTES {
            return Err(format!(
                "claim \"{id}\" would be larger than {} bytes",
                claim::MAX_CLAIM_BYTES
            ));
        }
        Ok((c, bytes))
    }

    /// The candidate ids, in order: the explicit id alone, or the slug, then
    /// `slug-2`, `slug-3`, and so on.
    fn candidate(&self, n: usize) -> Option<String> {
        match &self.explicit {
            Some(id) if n == 1 => Some(id.clone()),
            Some(_) => None,
            None if n == 1 => Some(self.slug.clone()),
            None if n < 1000 => Some(format!("{}-{n}", self.slug)),
            None => None,
        }
    }
}

/// `label: e. Nothing was written.`, without saying it twice.
fn nothing_written(label: &str, e: &str) -> String {
    let e = e.trim_end_matches('.');
    let e = e
        .strip_suffix("; nothing was written")
        .or_else(|| e.strip_suffix(". Nothing was written"))
        .unwrap_or(e);
    format!("{label}: {e}. Nothing was written.")
}

fn exists_message(id: &str) -> String {
    format!(
        "claim \"{id}\" already exists at .adsertio/claims/{id}.md. Use another id, or adsertio_attach to add evidence."
    )
}

/// Write one draft with `overwrite = false`. A made-up id that is taken moves
/// on to the next free one; `reserved` holds ids other drafts in this call
/// will use. Returns the id written and its pointer count.
fn write_draft(
    dir: &std::path::Path,
    d: &Draft,
    reserved: &[String],
) -> Result<(String, usize), String> {
    for n in 1.. {
        let Some(id) = d.candidate(n) else {
            break;
        };
        if d.explicit.is_none() && reserved.contains(&id) {
            continue;
        }
        let path = fsx::join_id(dir, &id, 1)?.with_extension("md");
        let (c, bytes) = d.claim(&id)?;
        match fsx::write_atomic(&path, &bytes, false) {
            Ok(()) => return Ok((id, c.evidence.len())),
            Err(e) if e.contains("already exists") && d.explicit.is_none() => continue,
            Err(e) if e.contains("already exists") => return Err(exists_message(&id)),
            Err(e) => return Err(format!("claim \"{id}\" not written: {e}")),
        }
    }
    Err("could not find a free id; pass id".into())
}

fn claim_add(a: &Args) -> Result<Value, String> {
    let store = Store::open()?;
    crate::store::claims_ready(&store, false)?;
    if a.has("claims") {
        return claim_batch(&store, a);
    }
    if !a.has("summary") {
        return Err(
            "missing required argument: summary. To record several claims, pass claims: [{summary, evidence}, ...] in one call."
                .into(),
        );
    }
    let d = draft(a)?;
    let (id, count) = write_draft(&store.claims_dir(), &d, &[])?;
    Ok(Value::obj(vec![
        ("id", Value::String(id)),
        ("evidence", Value::uint(count as u64)),
    ]))
}

/// `claims`: check every item, then write each with `overwrite = false`.
/// A bad item writes nothing. A write that fails part way says which were
/// written and which were not.
fn claim_batch(store: &Store, a: &Args) -> Result<Value, String> {
    if let Some(field) = CLAIM_FIELDS.iter().find(|p| a.has(p.name)) {
        return Err(format!(
            "pass claims, or the single-claim fields, not both (got claims and {}). Put {} inside each item.",
            field.name, field.name
        ));
    }
    let items = a.objects("claims");
    if items.is_empty() {
        return Err("claims is empty; pass one or more {summary, evidence} objects".into());
    }
    let dir = store.claims_dir();
    let mut drafts: Vec<Draft> = Vec::new();
    for (n, item) in items.iter().enumerate() {
        let label = format!("claims item {}", n + 1);
        let d = crate::tool::validate_fields("a claims item", CLAIM_FIELDS, item)
            .and_then(|args| draft(&args))
            .map_err(|e| nothing_written(&label, &e))?;
        if let Some(id) = &d.explicit {
            if let Some(first) = drafts
                .iter()
                .position(|o| o.explicit.as_deref() == Some(id.as_str()))
            {
                return Err(format!(
                    "{label}: id \"{id}\" is also claims item {}. Each claim needs its own id. Nothing was written.",
                    first + 1
                ));
            }
            let path = fsx::join_id(&dir, id, 1)?.with_extension("md");
            if std::fs::symlink_metadata(&path).is_ok() {
                return Err(nothing_written(&label, &exists_message(id)));
            }
        }
        d.claim(&d.slug).map_err(|e| nothing_written(&label, &e))?;
        drafts.push(d);
    }
    let mut reserved: Vec<String> = drafts.iter().filter_map(|d| d.explicit.clone()).collect();
    let mut written: Vec<String> = Vec::new();
    for (n, d) in drafts.iter().enumerate() {
        match write_draft(&dir, d, &reserved) {
            Ok((id, _)) => {
                reserved.push(id.clone());
                written.push(id);
            }
            Err(e) => {
                let rest: Vec<String> = drafts[n..]
                    .iter()
                    .enumerate()
                    .map(|(k, d)| match &d.explicit {
                        Some(id) => format!("item {} ({id})", n + k + 1),
                        None => format!("item {}", n + k + 1),
                    })
                    .collect();
                return Err(format!(
                    "claims item {} not written: {}. Written: [{}]. Not written: [{}]. Call adsertio_claim again with only the items not written.",
                    n + 1,
                    e.trim_end_matches('.'),
                    written.join(", "),
                    rest.join(", ")
                ));
            }
        }
    }
    Ok(Value::obj(vec![
        (
            "ids",
            Value::Array(written.iter().map(|id| Value::str(id.as_str())).collect()),
        ),
        ("count", Value::uint(written.len() as u64)),
    ]))
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

/// The pointers one attach adds: not empty, each valid, none repeated.
fn incoming_pointers(a: &Args) -> Result<Vec<String>, String> {
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
    Ok(incoming)
}

fn attach(a: &Args) -> Result<Value, String> {
    let store = Store::open()?;
    crate::store::claims_ready(&store, false)?;
    if a.has("items") {
        return attach_batch(&store, a);
    }
    for field in ["id", "evidence"] {
        if !a.has(field) {
            return Err(format!(
                "missing required argument: {field}. To add pointers to several claims, pass items: [{{id, evidence}}, ...] in one call."
            ));
        }
    }
    let id = a.str("id").unwrap_or("");
    let incoming = incoming_pointers(a)?;
    attach_one(&store, id, &incoming, true)
}

/// `items`: check every item against its claim, then attach each under that
/// claim's own lock. A bad item writes nothing.
fn attach_batch(store: &Store, a: &Args) -> Result<Value, String> {
    if let Some(field) = ATTACH_FIELDS.iter().find(|p| a.has(p.name)) {
        return Err(format!(
            "pass items, or id and evidence, not both (got items and {}). Put {} inside each item.",
            field.name, field.name
        ));
    }
    let items = a.objects("items");
    if items.is_empty() {
        return Err("items is empty; pass one or more {id, evidence} objects".into());
    }
    let mut plan: Vec<(String, Vec<String>)> = Vec::new();
    for (n, item) in items.iter().enumerate() {
        let label = format!("attach item {}", n + 1);
        let args = crate::tool::validate_fields("an attach item", ATTACH_FIELDS, item)
            .map_err(|e| nothing_written(&label, &e))?;
        let id = args.str("id").unwrap_or("").to_string();
        let incoming = incoming_pointers(&args).map_err(|e| nothing_written(&label, &e))?;
        if let Some(first) = plan.iter().position(|(other, _)| *other == id) {
            return Err(format!(
                "{label}: claim \"{}\" is also attach item {}. Put all of its pointers in one item. Nothing was written.",
                fsx::clip(&id, 60),
                first + 1
            ));
        }
        attach_one(store, &id, &incoming, false).map_err(|e| nothing_written(&label, &e))?;
        plan.push((id, incoming));
    }
    let mut done: Vec<Value> = Vec::new();
    let mut written: Vec<&str> = Vec::new();
    for (n, (id, incoming)) in plan.iter().enumerate() {
        match attach_one(store, id, incoming, true) {
            Ok(v) => {
                done.push(v);
                written.push(id);
            }
            Err(e) => {
                let rest: Vec<&str> = plan[n..].iter().map(|(id, _)| id.as_str()).collect();
                return Err(format!(
                    "attach item {} not written: {}. Updated: [{}]. Not updated: [{}]. Call adsertio_attach again with only the items not updated.",
                    n + 1,
                    e.trim_end_matches('.'),
                    written.join(", "),
                    rest.join(", ")
                ));
            }
        }
    }
    Ok(Value::obj(vec![("claims", Value::Array(done))]))
}

/// Add `incoming` to claim `id`. With `write` false this only checks, without
/// the lock, that the attach would succeed now.
fn attach_one(store: &Store, id: &str, incoming: &[String], write: bool) -> Result<Value, String> {
    let path = fsx::join_id(&store.claims_dir(), id, 1)?.with_extension("md");
    let body = || {
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
        for pointer in incoming {
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
        if write {
            fsx::write_atomic(&path, &bytes, true)
                .map_err(|e| format!("claim \"{id}\" not updated: {e}"))?;
        }
        Ok(Value::obj(vec![
            ("id", Value::str(&claim.id)),
            ("evidence", Value::uint(claim.evidence.len() as u64)),
            (
                "added",
                Value::Array(incoming.iter().map(|p| Value::str(p.as_str())).collect()),
            ),
        ]))
    };
    if write {
        // No lock file for an id with no claim: one lock file per claim, at most.
        if std::fs::symlink_metadata(&path).is_err() {
            return body();
        }
        fsx::with_claim_lock(&store.attach_lock(id)?, body)
    } else {
        body()
    }
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

fn doctor(a: &Args) -> Result<Value, String> {
    Ok(crate::doctor::run(a.bool("fix") == Some(true)))
}

pub fn store_check() -> Result<Value, String> {
    let store = Store::open()?;
    crate::store::claims_ready(&store, false)?;
    let found = claim_problems(&store, true, true)?;
    if !found.problems.is_empty() {
        return Err(found.problems.join("\n"));
    }
    Ok(Value::obj(vec![
        ("ok", Value::Bool(true)),
        ("claims", Value::uint(found.claims as u64)),
        (
            "stray_temp_files",
            Value::Array(found.strays.into_iter().map(Value::String).collect()),
        ),
    ]))
}

/// What `check` finds in the claims folder.
pub struct Findings {
    pub claims: usize,
    pub problems: Vec<String>,
    /// Temp files still there after the sweep (younger than an hour, or all
    /// of them when `clean` is false).
    pub strays: Vec<String>,
}

/// The claim checks `check` and `doctor` share. With `clean`, temp files
/// over an hour old and the lock files 1.3 left in `claims` are removed
/// first, as `check` does. With `evidence`, every pointer is read once for a
/// malformed timestamp or commit. `Err` is a claim that could not be read at
/// all. The lock files in `.adsertio/locks/` are not looked at.
pub fn claim_problems(store: &Store, clean: bool, evidence: bool) -> Result<Findings, String> {
    let mut problems = Vec::new();
    let mut strays = Vec::new();
    let dir = store.claims_dir();
    if clean {
        fsx::clear_abandoned(&dir);
    }
    if let Ok(rd) = std::fs::read_dir(&dir) {
        for entry in rd.flatten() {
            let name = entry.file_name().to_string_lossy().to_string();
            if fsx::is_old_lock(&name) || fsx::is_os_file(&name) {
                continue;
            }
            if fsx::is_work_file(&name) {
                strays.push(name);
                continue;
            }
            let slug = name.strip_suffix(".md");
            let kind = entry.file_type().ok();
            if kind.is_some_and(|kind| kind.is_symlink()) {
                problems.push(format!(
                    ".adsertio/claims/{name} is a symlink, which is not followed. Make it a real <id>.md file. It is left out of adsertio_view and adsertio_ls."
                ));
                continue;
            }
            let file = kind.is_some_and(|kind| kind.is_file());
            if !file || slug.is_none_or(|id| !fsx::valid_slug(id)) {
                problems.push(format!(
                    ".adsertio/claims/{name} is not a claim file. Use a slug id ending in .md. It is left out of adsertio_ls."
                ));
            }
        }
    }
    strays.sort();
    let claims = claim::load_all_with(store, clean)?;
    for claim in &claims {
        if !crate::fresh::valid_created(&claim.created) {
            problems.push(format!(
                "claim \"{}\" created time \"{}\" must be YYYY-MM-DDTHH:MM:SSZ",
                claim.id, claim.created
            ));
        }
    }
    if evidence {
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
    }
    Ok(Findings {
        claims: claims.len(),
        problems,
        strays,
    })
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
                "Call adsertio_view with states [\"unverified\", \"refuted\"] and compact true for the open claims (adsertio hook prints a session-start hook that does this), then adsertio_view for the ids that matter. Do not repeat a verdict from memory.",
            ),
        ),
        (
            "stale_after_commit",
            Value::str(
                "A Chisel or Trammel pass recorded before a commit is stale once HEAD moves, unless Chisel recorded the tested tree (Chisel 0.25.0 and later) and it is HEAD's tree: testing uncommitted work, then committing exactly that work, keeps the pass fresh. Otherwise rerun the tests at HEAD (for example chisel run -- <test command>), then view again.",
            ),
        ),
        (
            "problems",
            Value::str(
                "adsertio_doctor reports what is wrong with the store, damaged claims, git, or the evidence files, each with a fix. fix: true first removes temp and lock files whose writer is gone. adsertio check (CLI) fails on any damaged claim, for scripts and CI.",
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
                "Record all claims in one call: adsertio_claim with claims, a list of up to 50 {summary, evidence, id?}. Add pointers to several claims in one call: adsertio_attach with items, a list of {id, evidence}. Every item is checked first and a bad one writes nothing. Ids in one call must differ. adsertio_claim never overwrites. A repeated pointer is an error and the file is left unchanged. A claim holds at most 50 pointers. The body is at most 20000 bytes. The claim file is at most 262144 bytes. Duplicate tags and evidence on a new claim are kept once.",
            ),
        ),
    ]))
}
