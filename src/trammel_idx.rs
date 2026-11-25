//! One index of a Trammel snapshot, built while the JSON is scanned.
//!
//! A later selector is a map lookup. The scan keeps the fields a selector
//! reads (ids, `verification.passed`, `at`, `commit`, `active`) and does not
//! keep the rest of each row.

use crate::json::{JsonError, Kind, Number, Parser, Value};
use crate::read::{self, ReadError};
use std::collections::{HashMap, HashSet};

pub struct Stamp {
    pub at: Option<f64>,
    pub commits: Vec<String>,
    pub malformed: bool,
}

impl Stamp {
    fn empty() -> Stamp {
        Stamp {
            at: None,
            commits: Vec::new(),
            malformed: false,
        }
    }
}

pub enum StepKind {
    /// No row has this plan id and step id.
    NotInStore,
    /// The row is there and it has no `verification` yet.
    NotVerified,
    Bad,
    Fail,
    Pass,
}

pub struct StepHit {
    pub kind: StepKind,
    pub stamp: Stamp,
}

pub enum PlanKind {
    /// The plan id is not in `plans` and it has no steps.
    Unknown,
    /// The plan id is in `plans` and it has no steps.
    NoSteps,
    /// Some step has a `verification` that is not an object.
    Bad,
    Fail {
        failed: usize,
    },
    Waiting {
        waiting: usize,
    },
    Pass {
        steps: usize,
    },
}

pub struct PlanHit {
    pub kind: PlanKind,
    pub stamp: Stamp,
}

pub enum ConstraintKind {
    Missing,
    Active,
    Inactive,
    Bad,
}

enum Rows<T> {
    Absent,
    Bad(String),
    Ready(T),
}

enum StepRec {
    Absent,
    Bad,
    Fail(Stamp),
    Pass(Stamp),
}

struct StepData {
    by_id: HashMap<(i64, i64), StepRec>,
    plans: HashMap<i64, PlanAgg>,
}

struct PlanAgg {
    total: u32,
    failed: u32,
    waiting: u32,
    bad: bool,
    stamp: Stamp,
}

enum Active {
    Missing,
    On,
    Off,
    Bad,
}

struct Plans {
    ids: HashSet<i64>,
}

pub struct Index {
    steps: Rows<StepData>,
    plans: Rows<Plans>,
    constraints: Rows<HashMap<i64, Active>>,
    failures: Rows<HashSet<i64>>,
}

pub fn parse(text: &str, rel: &str) -> Result<Index, ReadError> {
    let mut parser = Parser::new(text);
    let (index, schema) = parse_document(&mut parser)
        .map_err(|error| ReadError::Unreadable(format!("{rel} is not valid JSON: {error}")))?;
    parser
        .finish()
        .map_err(|error| ReadError::Unreadable(format!("{rel} is not valid JSON: {error}")))?;
    let mut doc = Value::Object(Vec::new());
    if let Some(version) = schema {
        doc.insert("schema_version", Value::int(version));
    }
    read::check_trammel_schema(&doc, rel)?;
    Ok(index)
}

fn parse_document(parser: &mut Parser<'_>) -> Result<(Index, Option<i64>), JsonError> {
    parser.skip_ws();
    if !matches!(parser.kind()?, Kind::Object) {
        parser.skip_value(0)?;
        return Ok((empty_index(), None));
    }
    let mut schema: Option<i64> = None;
    let mut steps: Option<Rows<StepData>> = None;
    let mut plans: Option<Rows<Plans>> = None;
    let mut constraints: Option<Rows<HashMap<i64, Active>>> = None;
    let mut failures: Option<Rows<HashSet<i64>>> = None;
    parser.with_object(0, |parser, key, depth| {
        match key {
            "schema_version" => schema = take_i64(parser, depth)?,
            "steps" => steps = Some(take_steps(parser, depth)?),
            "plans" => plans = Some(take_plans(parser, depth)?),
            "constraints" => constraints = Some(take_constraints(parser, depth)?),
            "failures" => failures = Some(take_failures(parser, depth)?),
            _ => parser.skip_value(depth)?,
        }
        Ok(())
    })?;
    let mut index = empty_index();
    if let Some(rows) = steps {
        index.steps = rows;
    }
    if let Some(rows) = plans {
        index.plans = rows;
    }
    if let Some(rows) = constraints {
        index.constraints = rows;
    }
    if let Some(rows) = failures {
        index.failures = rows;
    }
    Ok((index, schema))
}

fn empty_index() -> Index {
    Index {
        steps: Rows::Absent,
        plans: Rows::Absent,
        constraints: Rows::Absent,
        failures: Rows::Absent,
    }
}

fn take_i64(parser: &mut Parser<'_>, depth: usize) -> Result<Option<i64>, JsonError> {
    if matches!(parser.kind()?, Kind::Number) {
        Ok(parser.take_number()?.as_i64())
    } else {
        parser.skip_value(depth)?;
        Ok(None)
    }
}

fn take_steps(parser: &mut Parser<'_>, depth: usize) -> Result<Rows<StepData>, JsonError> {
    if !matches!(parser.kind()?, Kind::Array) {
        parser.skip_value(depth)?;
        return Ok(Rows::Bad(missing("steps")));
    }
    let mut data = StepData {
        by_id: HashMap::new(),
        plans: HashMap::new(),
    };
    let mut poison: Option<String> = None;
    parser.with_array(depth, |parser, index| {
        if !matches!(parser.kind()?, Kind::Object) {
            if poison.is_none() {
                poison = Some(not_object("steps", index));
            }
            return parser.skip_value(depth + 1);
        }
        let raw = parse_step(parser, depth + 1)?;
        if poison.is_some() {
            return Ok(());
        }
        if let Some(plan_id) = raw.plan_id {
            let agg = data.plans.entry(plan_id).or_insert_with(|| PlanAgg {
                total: 0,
                failed: 0,
                waiting: 0,
                bad: false,
                stamp: Stamp::empty(),
            });
            agg.total += 1;
            match &raw.rec {
                StepRec::Bad => agg.bad = true,
                StepRec::Absent => agg.waiting += 1,
                StepRec::Fail(stamp) => {
                    agg.failed += 1;
                    merge_stamp(&mut agg.stamp, stamp);
                }
                StepRec::Pass(stamp) => merge_stamp(&mut agg.stamp, stamp),
            }
        }
        if let (Some(plan_id), Some(id)) = (raw.plan_id, raw.id) {
            data.by_id.entry((plan_id, id)).or_insert(raw.rec);
        }
        Ok(())
    })?;
    Ok(match poison {
        Some(message) => Rows::Bad(message),
        None => Rows::Ready(data),
    })
}

struct RawStep {
    id: Option<i64>,
    plan_id: Option<i64>,
    rec: StepRec,
}

fn parse_step(parser: &mut Parser<'_>, depth: usize) -> Result<RawStep, JsonError> {
    let mut id = None;
    let mut plan_id = None;
    let mut ver: Option<Ver> = None;
    parser.with_object(depth, |parser, key, depth| {
        match key {
            "id" => id = take_i64(parser, depth)?,
            "plan_id" => plan_id = take_i64(parser, depth)?,
            "verification" => ver = Some(parse_verification(parser, depth)?),
            _ => parser.skip_value(depth)?,
        }
        Ok(())
    })?;
    let rec = match ver {
        None => StepRec::Absent,
        Some(Ver::Bad) => StepRec::Bad,
        Some(Ver::Object { pass, stamp }) => {
            if pass {
                StepRec::Pass(stamp)
            } else {
                StepRec::Fail(stamp)
            }
        }
    };
    Ok(RawStep { id, plan_id, rec })
}

enum Ver {
    Bad,
    Object { pass: bool, stamp: Stamp },
}

fn parse_verification(parser: &mut Parser<'_>, depth: usize) -> Result<Ver, JsonError> {
    if !matches!(parser.kind()?, Kind::Object) {
        parser.skip_value(depth)?;
        return Ok(Ver::Bad);
    }
    let mut passed = false;
    let mut at: Option<Number> = None;
    let mut at_seen = false;
    let mut commit: Option<String> = None;
    let mut commit_bad = false;
    let mut commit_seen = false;
    parser.with_object(depth, |parser, key, depth| {
        match key {
            "passed" => {
                if matches!(parser.kind()?, Kind::Bool) {
                    passed = parser.take_bool()?;
                } else {
                    parser.skip_value(depth)?;
                }
            }
            "at" => {
                at_seen = true;
                if matches!(parser.kind()?, Kind::Number) {
                    at = Some(parser.take_number()?);
                } else {
                    parser.skip_value(depth)?;
                }
            }
            "commit" => {
                commit_seen = true;
                if matches!(parser.kind()?, Kind::String) {
                    commit = Some(parser.take_string()?);
                } else {
                    commit_bad = true;
                    parser.skip_value(depth)?;
                }
            }
            _ => parser.skip_value(depth)?,
        }
        Ok(())
    })?;
    Ok(Ver::Object {
        pass: passed,
        stamp: stamp_of(at_seen, at, commit_seen, commit, commit_bad),
    })
}

fn stamp_of(
    at_seen: bool,
    at: Option<Number>,
    commit_seen: bool,
    commit: Option<String>,
    commit_bad: bool,
) -> Stamp {
    let at_value = at.as_ref().map(Number::as_value);
    let (at, bad_time) = crate::fresh::read_time(if at_seen {
        at_value.as_ref().or(Some(&Value::Bool(false)))
    } else {
        None
    });
    // A present non-number is not a finite timestamp. Bool stands in for
    // "present, not a number" when the token was not a number at all.
    let (at, bad_time) = if at_seen && at_value.is_none() {
        (None, true)
    } else {
        (at, bad_time)
    };
    if bad_time {
        return Stamp {
            at: None,
            commits: Vec::new(),
            malformed: true,
        };
    }
    let commit_value = match (commit_seen, commit) {
        (false, _) => None,
        (true, Some(text)) => Some(Value::String(text)),
        (true, None) => Some(Value::Bool(false)),
    };
    let (commit, bad_commit) = if commit_bad {
        (None, true)
    } else {
        crate::fresh::read_commit(commit_value.as_ref())
    };
    if bad_commit {
        return Stamp {
            at,
            commits: Vec::new(),
            malformed: true,
        };
    }
    Stamp {
        at,
        commits: commit.into_iter().collect(),
        malformed: false,
    }
}

fn merge_stamp(into: &mut Stamp, step: &Stamp) {
    into.at = older(into.at, step.at);
    into.commits.extend(step.commits.iter().cloned());
    into.malformed |= step.malformed;
}

fn older(left: Option<f64>, right: Option<f64>) -> Option<f64> {
    match (left, right) {
        (Some(a), Some(b)) => Some(a.min(b)),
        (Some(a), None) => Some(a),
        (None, other) => other,
    }
}

fn take_plans(parser: &mut Parser<'_>, depth: usize) -> Result<Rows<Plans>, JsonError> {
    if !matches!(parser.kind()?, Kind::Array) {
        parser.skip_value(depth)?;
        return Ok(Rows::Bad(missing("plans")));
    }
    let mut ids = HashSet::new();
    let mut poison: Option<String> = None;
    parser.with_array(depth, |parser, index| {
        if !matches!(parser.kind()?, Kind::Object) {
            if poison.is_none() {
                poison = Some(not_object("plans", index));
            }
            return parser.skip_value(depth + 1);
        }
        let mut id = None;
        parser.with_object(depth + 1, |parser, key, depth| {
            if key == "id" {
                id = take_i64(parser, depth)?;
            } else {
                parser.skip_value(depth)?;
            }
            Ok(())
        })?;
        if poison.is_none() {
            if let Some(id) = id {
                ids.insert(id);
            }
        }
        Ok(())
    })?;
    Ok(match poison {
        Some(message) => Rows::Bad(message),
        None => Rows::Ready(Plans { ids }),
    })
}

fn take_constraints(
    parser: &mut Parser<'_>,
    depth: usize,
) -> Result<Rows<HashMap<i64, Active>>, JsonError> {
    if !matches!(parser.kind()?, Kind::Array) {
        parser.skip_value(depth)?;
        return Ok(Rows::Bad(missing("constraints")));
    }
    let mut rows = HashMap::new();
    let mut poison: Option<String> = None;
    parser.with_array(depth, |parser, index| {
        if !matches!(parser.kind()?, Kind::Object) {
            if poison.is_none() {
                poison = Some(not_object("constraints", index));
            }
            return parser.skip_value(depth + 1);
        }
        let mut id = None;
        let mut active = Active::Missing;
        parser.with_object(depth + 1, |parser, key, depth| {
            match key {
                "id" => id = take_i64(parser, depth)?,
                "active" => active = take_active(parser, depth)?,
                _ => parser.skip_value(depth)?,
            }
            Ok(())
        })?;
        if poison.is_none() {
            if let Some(id) = id {
                rows.entry(id).or_insert(active);
            }
        }
        Ok(())
    })?;
    Ok(match poison {
        Some(message) => Rows::Bad(message),
        None => Rows::Ready(rows),
    })
}

fn take_active(parser: &mut Parser<'_>, depth: usize) -> Result<Active, JsonError> {
    if matches!(parser.kind()?, Kind::Bool) {
        Ok(if parser.take_bool()? {
            Active::On
        } else {
            Active::Off
        })
    } else {
        parser.skip_value(depth)?;
        Ok(Active::Bad)
    }
}

fn take_failures(parser: &mut Parser<'_>, depth: usize) -> Result<Rows<HashSet<i64>>, JsonError> {
    if !matches!(parser.kind()?, Kind::Array) {
        parser.skip_value(depth)?;
        return Ok(Rows::Bad(missing("failures")));
    }
    let mut ids = HashSet::new();
    let mut poison: Option<String> = None;
    parser.with_array(depth, |parser, index| {
        if !matches!(parser.kind()?, Kind::Object) {
            if poison.is_none() {
                poison = Some(not_object("failures", index));
            }
            return parser.skip_value(depth + 1);
        }
        let mut id = None;
        parser.with_object(depth + 1, |parser, key, depth| {
            if key == "id" {
                id = take_i64(parser, depth)?;
            } else {
                parser.skip_value(depth)?;
            }
            Ok(())
        })?;
        if poison.is_none() {
            if let Some(id) = id {
                ids.insert(id);
            }
        }
        Ok(())
    })?;
    Ok(match poison {
        Some(message) => Rows::Bad(message),
        None => Rows::Ready(ids),
    })
}

fn missing(name: &str) -> String {
    format!("missing {name}, or it is not an array. A damaged store is not treated as empty")
}

fn not_object(name: &str, index: usize) -> String {
    format!("{name}[{index}] is not an object")
}

impl Index {
    pub fn step(&self, plan: i64, step: i64) -> Result<StepHit, String> {
        let data = ready(&self.steps, "steps")?;
        let Some(rec) = data.by_id.get(&(plan, step)) else {
            return Ok(StepHit {
                kind: StepKind::NotInStore,
                stamp: Stamp::empty(),
            });
        };
        let (kind, stamp) = match rec {
            StepRec::Absent => (StepKind::NotVerified, Stamp::empty()),
            StepRec::Bad => (StepKind::Bad, Stamp::empty()),
            StepRec::Fail(stamp) => (StepKind::Fail, clone_stamp(stamp)),
            StepRec::Pass(stamp) => (StepKind::Pass, clone_stamp(stamp)),
        };
        Ok(StepHit { kind, stamp })
    }

    pub fn plan(&self, plan: i64) -> Result<PlanHit, String> {
        let data = ready(&self.steps, "steps")?;
        let Some(agg) = data.plans.get(&plan) else {
            let known = match &self.plans {
                Rows::Ready(plans) => plans.ids.contains(&plan),
                Rows::Absent => return Err(missing("plans")),
                Rows::Bad(message) => return Err(message.clone()),
            };
            return Ok(PlanHit {
                kind: if known {
                    PlanKind::NoSteps
                } else {
                    PlanKind::Unknown
                },
                stamp: Stamp::empty(),
            });
        };
        if agg.bad {
            return Ok(PlanHit {
                kind: PlanKind::Bad,
                stamp: Stamp::empty(),
            });
        }
        let kind = if agg.failed > 0 {
            PlanKind::Fail {
                failed: agg.failed as usize,
            }
        } else if agg.waiting > 0 {
            PlanKind::Waiting {
                waiting: agg.waiting as usize,
            }
        } else {
            PlanKind::Pass {
                steps: agg.total as usize,
            }
        };
        Ok(PlanHit {
            kind,
            stamp: clone_stamp(&agg.stamp),
        })
    }

    pub fn constraint(&self, id: i64) -> Result<ConstraintKind, String> {
        let rows = ready(&self.constraints, "constraints")?;
        Ok(match rows.get(&id) {
            None => ConstraintKind::Missing,
            Some(Active::Missing | Active::On) => ConstraintKind::Active,
            Some(Active::Off) => ConstraintKind::Inactive,
            Some(Active::Bad) => ConstraintKind::Bad,
        })
    }

    pub fn failure(&self, id: i64) -> Result<bool, String> {
        let rows = ready(&self.failures, "failures")?;
        Ok(rows.contains(&id))
    }
}

fn ready<'a, T>(rows: &'a Rows<T>, name: &str) -> Result<&'a T, String> {
    match rows {
        Rows::Ready(value) => Ok(value),
        Rows::Absent => Err(missing(name)),
        Rows::Bad(message) => Err(message.clone()),
    }
}

fn clone_stamp(stamp: &Stamp) -> Stamp {
    Stamp {
        at: stamp.at,
        commits: stamp.commits.clone(),
        malformed: stamp.malformed,
    }
}
