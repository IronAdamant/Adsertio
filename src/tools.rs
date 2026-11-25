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
            Param { name: "verdict", kind: Kind::Str, required: false, doc: "Only claims with this verdict.", choices: &["verified", "refuted", "unverified"] },
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
        description: "List claim ids, summaries, and how many pointers each has. Does not read the evidence and does not report a verdict; call adsertio_view for that.",
        params: &[],
        run: ls,
    },
];

fn claim_add(a: &Args) -> Result<Value, String> {
    let store = Store::open()?;
    let summary = a.str("summary").unwrap_or_default();
    claim::validate_summary(summary)?;
    let evidence: Vec<String> = a.list("evidence").iter().map(|s| s.to_string()).collect();
    for p in &evidence {
        claim::valid_pointer(p)?;
    }
    let tags: Vec<String> = a.list("tags").iter().map(|s| s.to_string()).collect();
    if let Some(bad) = tags.iter().find(|t| !fsx::valid_slug(t)) {
        return Err(format!("bad tag \"{}\": use a slug", fsx::clip(bad, 40)));
    }
    let author = a.str("author").unwrap_or("agent");
    if !fsx::valid_slug(author) {
        return Err(format!(
            "invalid author \"{}\": use a slug",
            fsx::clip(author, 40)
        ));
    }
    let dir = store.claims_dir();
    let id = match a.str("id") {
        Some(id) => id.to_string(),
        None => unique_id(&dir, summary)?,
    };
    let path = fsx::join_id(&dir, &id, 1)?.with_extension("md");
    fsx::make_private_dir(&dir)?;
    let c = Claim {
        id: id.clone(),
        summary: summary.to_string(),
        author: author.to_string(),
        created: fsx::now_iso(),
        evidence,
        tags,
        body: a.str("body").unwrap_or_default().to_string(),
    };
    fsx::write_atomic(&path, c.render().as_bytes(), false)
        .map_err(|e| format!("claim \"{id}\" not written: {e}"))?;
    Ok(Value::obj(vec![
        ("id", Value::String(id)),
        ("evidence", Value::uint(c.evidence.len() as u64)),
    ]))
}

fn unique_id(dir: &std::path::Path, summary: &str) -> Result<String, String> {
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
    let slug = if slug.is_empty() {
        "claim".to_string()
    } else {
        slug
    };
    for n in 1..1000 {
        let id = if n == 1 {
            slug.clone()
        } else {
            format!("{slug}-{n}")
        };
        if !dir.join(format!("{id}.md")).exists() {
            return Ok(id);
        }
    }
    Err("could not find a free id; pass id".into())
}

fn attach(a: &Args) -> Result<Value, String> {
    let store = Store::open()?;
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
    let meta = match std::fs::symlink_metadata(&path) {
        Ok(meta) => meta,
        Err(_) => return Err(format!("no claim with id \"{}\"", fsx::clip(id, 60))),
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
    claim.evidence.extend(incoming);
    fsx::write_atomic(&path, claim.render().as_bytes(), true)
        .map_err(|e| format!("claim \"{id}\" not updated: {e}"))?;
    Ok(Value::obj(vec![
        ("id", Value::str(&claim.id)),
        ("evidence", Value::uint(claim.evidence.len() as u64)),
    ]))
}

fn ls(_a: &Args) -> Result<Value, String> {
    let store = Store::open()?;
    let claims = claim::load_all(&store)?;
    let rows = claims
        .iter()
        .map(|claim| {
            Value::obj(vec![
                ("id", Value::str(&claim.id)),
                ("summary", Value::str(&claim.summary)),
                ("pointers", Value::uint(claim.evidence.len() as u64)),
            ])
        })
        .collect();
    Ok(Value::obj(vec![
        ("claims", Value::Array(rows)),
        ("count", Value::uint(claims.len() as u64)),
    ]))
}

fn view(a: &Args) -> Result<Value, String> {
    let store = Store::open()?;
    let mut claims = claim::load_all(&store)?;
    if let Some(id) = a.str("id") {
        claims.retain(|c| c.id == id);
        if claims.is_empty() {
            return Err(format!("no claim with id \"{}\"", fsx::clip(id, 60)));
        }
    }
    let mut rows = Vec::new();
    let (mut verified, mut refuted, mut unverified) = (0u64, 0u64, 0u64);
    for c in &claims {
        let v = verdict::view(&store.project, c);
        match v.verdict {
            verdict::Verdict::Verified => verified += 1,
            verdict::Verdict::Refuted => refuted += 1,
            verdict::Verdict::Unverified => unverified += 1,
        }
        if a.str("verdict").is_none_or(|w| w == v.verdict.name()) {
            rows.push(v.to_value(c));
        }
    }
    Ok(Value::obj(vec![
        ("claims", Value::Array(rows)),
        (
            "counts",
            Value::obj(vec![
                ("verified", Value::uint(verified)),
                ("refuted", Value::uint(refuted)),
                ("unverified", Value::uint(unverified)),
            ]),
        ),
    ]))
}
