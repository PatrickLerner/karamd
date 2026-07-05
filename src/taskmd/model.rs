//! The full taskmd task model.
//!
//! A [`Task`] is backed by its complete frontmatter as an *ordered* YAML
//! mapping (`doc`), plus the raw markdown body. Typed accessors parse out of
//! the mapping on demand and typed mutators write back into it, so a
//! parse → mutate → serialize round trip preserves every field karamd does not
//! know about (the spec requires unknown fields to be ignored *and preserved*)
//! and keeps the original key order. Only the fields karamd actually changes
//! change.

use anyhow::{Result, bail};
use chrono::NaiveDate;
use serde::Serialize;
use serde_norway::{Mapping, Value};
use std::path::PathBuf;

/// Task status, exactly the spec enum. Note `completed` (not `done`) and the
/// hyphenated `in-progress` / `in-review`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum Status {
    Pending,
    InProgress,
    InReview,
    Completed,
    Blocked,
    Cancelled,
}

impl Status {
    pub const ALL: [Status; 6] = [
        Status::Pending,
        Status::InProgress,
        Status::InReview,
        Status::Completed,
        Status::Blocked,
        Status::Cancelled,
    ];

    pub fn as_str(self) -> &'static str {
        match self {
            Status::Pending => "pending",
            Status::InProgress => "in-progress",
            Status::InReview => "in-review",
            Status::Completed => "completed",
            Status::Blocked => "blocked",
            Status::Cancelled => "cancelled",
        }
    }

    pub fn parse(s: &str) -> Option<Status> {
        Status::ALL.into_iter().find(|v| v.as_str() == s)
    }

    /// Terminal statuses end an occurrence; everything else counts as open.
    pub fn is_terminal(self) -> bool {
        matches!(self, Status::Completed | Status::Cancelled)
    }
}

/// Priority per spec. Ordered so query comparisons (`priority>=high`) work.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Priority {
    Low,
    Medium,
    High,
    Critical,
}

impl Priority {
    pub const ALL: [Priority; 4] = [
        Priority::Low,
        Priority::Medium,
        Priority::High,
        Priority::Critical,
    ];

    pub fn as_str(self) -> &'static str {
        match self {
            Priority::Low => "low",
            Priority::Medium => "medium",
            Priority::High => "high",
            Priority::Critical => "critical",
        }
    }

    pub fn parse(s: &str) -> Option<Priority> {
        Priority::ALL.into_iter().find(|v| v.as_str() == s)
    }
}

/// Effort per spec. Ordered so `effort<=medium` style comparisons work.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Effort {
    Small,
    Medium,
    Large,
}

impl Effort {
    pub const ALL: [Effort; 3] = [Effort::Small, Effort::Medium, Effort::Large];

    pub fn as_str(self) -> &'static str {
        match self {
            Effort::Small => "small",
            Effort::Medium => "medium",
            Effort::Large => "large",
        }
    }

    pub fn parse(s: &str) -> Option<Effort> {
        Effort::ALL.into_iter().find(|v| v.as_str() == s)
    }
}

/// Work-item classification per spec (the `type` frontmatter field).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum TaskType {
    Feature,
    Bug,
    Improvement,
    Chore,
    Docs,
}

impl TaskType {
    pub const ALL: [TaskType; 5] = [
        TaskType::Feature,
        TaskType::Bug,
        TaskType::Improvement,
        TaskType::Chore,
        TaskType::Docs,
    ];

    pub fn as_str(self) -> &'static str {
        match self {
            TaskType::Feature => "feature",
            TaskType::Bug => "bug",
            TaskType::Improvement => "improvement",
            TaskType::Chore => "chore",
            TaskType::Docs => "docs",
        }
    }

    pub fn parse(s: &str) -> Option<TaskType> {
        TaskType::ALL.into_iter().find(|v| v.as_str() == s)
    }
}

/// One typed `verify:` acceptance check. Unknown types are kept in the doc
/// verbatim (round-trip) and surface here as [`VerifyCheck::Unknown`].
#[derive(Debug, Clone, PartialEq)]
pub enum VerifyCheck {
    Bash { run: String, dir: Option<String> },
    Assert { check: String },
    Unknown { kind: Option<String> },
}

/// Splitting a file into candidate tasks and everything else.
///
/// The scanner must never mistake the spec doc, READMEs, templates, or fenced
/// ```yaml``` examples for tasks — only a *leading* `---` fence with a YAML
/// mapping that carries task keys counts.
#[derive(Debug, PartialEq)]
pub enum ParseOutcome {
    Task(Task),
    /// No leading frontmatter fence, or the fence does not delimit a YAML
    /// mapping that looks like a task. Silently ignored by the scanner.
    NotATask,
    /// Meant to be a task (has frontmatter with task keys) but is broken:
    /// malformed YAML, or missing/empty `id` or `title`. The scanner skips it;
    /// `validate` reports it.
    Invalid(String),
}

/// Split `content` into (frontmatter yaml, raw body after the closing fence).
/// Tolerates CRLF: a synced vault picks it up, and an LF-only parser would
/// silently drop fields. The closing fence must be a line that is exactly
/// `---` (optionally with a trailing `\r`).
pub fn split_frontmatter(content: &str) -> Option<(&str, &str)> {
    let rest = content
        .strip_prefix("---\r\n")
        .or_else(|| content.strip_prefix("---\n"))?;
    let mut offset = 0;
    for line in rest.split_inclusive('\n') {
        let trimmed = line.trim_end_matches(['\n', '\r']);
        if trimmed == "---" {
            let fm = &rest[..offset];
            let body = &rest[offset + line.len()..];
            return Some((fm, body));
        }
        offset += line.len();
    }
    None
}

/// Read a frontmatter value leniently as a string: strings pass through,
/// numbers stringify (an unquoted `id: 42` must not vanish), everything else
/// is not a string.
fn value_str(v: &Value) -> Option<String> {
    match v {
        Value::String(s) => Some(s.clone()),
        Value::Number(n) => Some(n.to_string()),
        Value::Bool(b) => Some(b.to_string()),
        _ => None,
    }
}

/// Parse a `YYYY-MM-DD` scalar, tolerating quotes and stray whitespace
/// (including `\r` from CRLF files).
fn value_date(v: &Value) -> Option<NaiveDate> {
    let s = value_str(v)?;
    NaiveDate::parse_from_str(s.trim().trim_matches('"').trim(), "%Y-%m-%d").ok()
}

/// A single task: full frontmatter (`doc`, ordered, source of truth) + body.
#[derive(Debug, Clone, PartialEq)]
pub struct Task {
    doc: Mapping,
    /// Raw markdown after the closing fence, byte-preserved on round trip
    /// (including its leading blank line).
    pub body: String,
    /// Path relative to the tasks dir; set by the scanner, `None` for a task
    /// not yet written.
    pub rel_path: Option<PathBuf>,
    /// Group derived from the parent directory; explicit `group:` wins.
    pub dir_group: Option<String>,
}

impl Task {
    /// A fresh task with the canonical minimal frontmatter karamd emits.
    pub fn new(id: &str, title: &str, today: NaiveDate) -> Task {
        let mut doc = Mapping::new();
        doc.insert("id".into(), Value::String(id.to_string()));
        doc.insert("title".into(), Value::String(title.to_string()));
        doc.insert("status".into(), Value::String("pending".into()));
        doc.insert(
            "created_at".into(),
            Value::String(today.format("%Y-%m-%d").to_string()),
        );
        Task {
            doc,
            body: format!("\n# {title}\n"),
            rel_path: None,
            dir_group: None,
        }
    }

    /// Classify file content per the scanner contract (see [`ParseOutcome`]).
    pub fn parse(content: &str) -> ParseOutcome {
        let Some((fm, body)) = split_frontmatter(content) else {
            return ParseOutcome::NotATask;
        };
        let doc: Mapping = match serde_norway::from_str::<Value>(fm) {
            Ok(Value::Mapping(m)) => m,
            // Frontmatter that isn't a mapping (a doc using `---` rulers).
            Ok(_) => return ParseOutcome::NotATask,
            Err(e) => return ParseOutcome::Invalid(format!("malformed frontmatter: {e}")),
        };
        // Only mappings that *look like* a task are task candidates; a Jekyll
        // style doc with unrelated frontmatter is not our business.
        let looks_like_task = ["id", "title", "status"]
            .iter()
            .any(|k| doc.contains_key(Value::String((*k).to_string())));
        if !looks_like_task {
            return ParseOutcome::NotATask;
        }
        let has = |k: &str| {
            doc.get(Value::String(k.to_string()))
                .and_then(value_str)
                .is_some_and(|s| !s.trim().is_empty())
        };
        if !has("id") {
            return ParseOutcome::Invalid("missing required field `id`".into());
        }
        if !has("title") {
            return ParseOutcome::Invalid("missing required field `title`".into());
        }
        ParseOutcome::Task(Task {
            doc,
            body: body.to_string(),
            rel_path: None,
            dir_group: None,
        })
    }

    /// Like [`Task::parse`] but for callers that need a task or an error.
    pub fn parse_required(content: &str) -> Result<Task> {
        match Task::parse(content) {
            ParseOutcome::Task(t) => Ok(t),
            ParseOutcome::NotATask => bail!("not a task file (no task frontmatter)"),
            ParseOutcome::Invalid(reason) => bail!("invalid task file: {reason}"),
        }
    }

    /// Serialize back to markdown. Frontmatter comes from `doc` in order, so
    /// unknown fields and their positions survive; the body is byte-preserved.
    pub fn to_markdown(&self) -> String {
        let fm = serde_norway::to_string(&self.doc).expect("frontmatter mapping serializes");
        format!("---\n{fm}---\n{}", self.body)
    }

    // ---- raw doc access ----

    pub fn get(&self, key: &str) -> Option<&Value> {
        self.doc.get(Value::String(key.to_string()))
    }

    /// Upsert a frontmatter value, preserving an existing key's position.
    pub fn set(&mut self, key: &str, value: Value) {
        self.doc.insert(Value::String(key.to_string()), value);
    }

    pub fn remove(&mut self, key: &str) {
        self.doc.remove(Value::String(key.to_string()));
    }

    fn get_str(&self, key: &str) -> Option<String> {
        self.get(key).and_then(value_str)
    }

    fn get_list(&self, key: &str) -> Vec<String> {
        match self.get(key) {
            Some(Value::Sequence(seq)) => seq.iter().filter_map(value_str).collect(),
            // A scalar where a list belongs still reads as one entry.
            Some(v) => value_str(v).into_iter().collect(),
            None => Vec::new(),
        }
    }

    // ---- typed accessors ----

    /// Guaranteed non-empty by [`Task::parse`] / [`Task::new`].
    pub fn id(&self) -> String {
        self.get_str("id").unwrap_or_default()
    }

    pub fn title(&self) -> String {
        self.get_str("title").unwrap_or_default()
    }

    pub fn status_raw(&self) -> Option<String> {
        self.get_str("status")
    }

    pub fn status(&self) -> Option<Status> {
        self.status_raw().as_deref().and_then(Status::parse)
    }

    /// Missing status means `pending` per spec ("initial state").
    pub fn effective_status(&self) -> Status {
        self.status().unwrap_or(Status::Pending)
    }

    pub fn priority_raw(&self) -> Option<String> {
        self.get_str("priority")
    }

    pub fn priority(&self) -> Option<Priority> {
        self.priority_raw().as_deref().and_then(Priority::parse)
    }

    /// Missing priority means `medium` per spec ("default").
    pub fn effective_priority(&self) -> Priority {
        self.priority().unwrap_or(Priority::Medium)
    }

    pub fn effort_raw(&self) -> Option<String> {
        self.get_str("effort")
    }

    pub fn effort(&self) -> Option<Effort> {
        self.effort_raw().as_deref().and_then(Effort::parse)
    }

    pub fn task_type_raw(&self) -> Option<String> {
        self.get_str("type")
    }

    pub fn task_type(&self) -> Option<TaskType> {
        self.task_type_raw().as_deref().and_then(TaskType::parse)
    }

    pub fn dependencies(&self) -> Vec<String> {
        self.get_list("dependencies")
    }

    pub fn tags(&self) -> Vec<String> {
        self.get_list("tags")
    }

    pub fn touches(&self) -> Vec<String> {
        self.get_list("touches")
    }

    pub fn context(&self) -> Vec<String> {
        self.get_list("context")
    }

    pub fn pr(&self) -> Vec<String> {
        self.get_list("pr")
    }

    /// Explicit `group:` wins; otherwise the scanner-derived directory group.
    pub fn group(&self) -> Option<String> {
        self.get_str("group").or_else(|| self.dir_group.clone())
    }

    pub fn owner(&self) -> Option<String> {
        self.get_str("owner")
    }

    pub fn phase(&self) -> Option<String> {
        self.get_str("phase")
    }

    pub fn parent(&self) -> Option<String> {
        self.get_str("parent")
    }

    pub fn external_id(&self) -> Option<String> {
        self.get_str("external_id")
    }

    /// karamd's own recurring-rule dedup marker.
    pub fn recurring(&self) -> Option<String> {
        self.get_str("recurring")
    }

    /// `created_at`, accepting the deprecated `created` alias.
    pub fn created_at(&self) -> Option<NaiveDate> {
        self.get("created_at")
            .or_else(|| self.get("created"))
            .and_then(value_date)
    }

    pub fn completed_at(&self) -> Option<NaiveDate> {
        self.get("completed_at").and_then(value_date)
    }

    pub fn cancelled_at(&self) -> Option<NaiveDate> {
        self.get("cancelled_at").and_then(value_date)
    }

    /// The `due` target date, verbatim (may be malformed; `validate` flags it).
    pub fn due_raw(&self) -> Option<String> {
        self.get_str("due")
    }

    /// The `due` target date, parsed; `None` if absent or not `YYYY-MM-DD`.
    pub fn due(&self) -> Option<NaiveDate> {
        self.get("due").and_then(value_date)
    }

    pub fn verify(&self) -> Vec<VerifyCheck> {
        let Some(Value::Sequence(seq)) = self.get("verify") else {
            return Vec::new();
        };
        seq.iter()
            .map(|entry| {
                let get = |k: &str| {
                    entry
                        .as_mapping()
                        .and_then(|m| m.get(Value::String(k.to_string())))
                        .and_then(value_str)
                };
                match get("type").as_deref() {
                    Some("bash") if get("run").is_some() => VerifyCheck::Bash {
                        run: get("run").expect("checked"),
                        dir: get("dir"),
                    },
                    Some("assert") if get("check").is_some() => VerifyCheck::Assert {
                        check: get("check").expect("checked"),
                    },
                    kind => VerifyCheck::Unknown {
                        kind: kind.map(str::to_string),
                    },
                }
            })
            .collect()
    }

    // ---- typed mutators ----

    pub fn set_title(&mut self, title: &str) {
        self.set("title", Value::String(title.to_string()));
    }

    /// Change status and maintain the auto timestamps per spec: `completed_at`
    /// is set on the transition *to* `completed` and cleared when status moves
    /// away; `cancelled_at` likewise. Re-setting the same terminal status keeps
    /// the original date (idempotent).
    pub fn set_status(&mut self, status: Status, today: NaiveDate) {
        let was = self.status();
        self.set("status", Value::String(status.as_str().to_string()));
        let date = Value::String(today.format("%Y-%m-%d").to_string());
        match status {
            Status::Completed => {
                if was != Some(Status::Completed) {
                    self.set("completed_at", date);
                }
                self.remove("cancelled_at");
            }
            Status::Cancelled => {
                if was != Some(Status::Cancelled) {
                    self.set("cancelled_at", date);
                }
                self.remove("completed_at");
            }
            _ => {
                self.remove("completed_at");
                self.remove("cancelled_at");
            }
        }
    }

    pub fn set_priority(&mut self, p: Priority) {
        self.set("priority", Value::String(p.as_str().to_string()));
    }

    pub fn set_effort(&mut self, e: Effort) {
        self.set("effort", Value::String(e.as_str().to_string()));
    }

    pub fn set_task_type(&mut self, t: TaskType) {
        self.set("type", Value::String(t.as_str().to_string()));
    }

    pub fn set_phase(&mut self, phase: Option<&str>) {
        match phase {
            Some(p) => self.set("phase", Value::String(p.to_string())),
            None => self.remove("phase"),
        }
    }

    pub fn set_owner(&mut self, owner: Option<&str>) {
        match owner {
            Some(o) => self.set("owner", Value::String(o.to_string())),
            None => self.remove("owner"),
        }
    }

    /// Set (or clear, with `None`) the `due` target date. Stored verbatim;
    /// callers validate the `YYYY-MM-DD` shape before writing.
    pub fn set_due(&mut self, due: Option<&str>) {
        match due {
            Some(d) => self.set("due", Value::String(d.to_string())),
            None => self.remove("due"),
        }
    }

    pub fn set_parent(&mut self, parent: Option<&str>) {
        match parent {
            Some(p) => self.set("parent", Value::String(p.to_string())),
            None => self.remove("parent"),
        }
    }

    fn set_list(&mut self, key: &str, items: &[String]) {
        let seq = items
            .iter()
            .map(|s| Value::String(s.clone()))
            .collect::<Vec<_>>();
        self.set(key, Value::Sequence(seq));
    }

    pub fn set_tags(&mut self, tags: &[String]) {
        self.set_list("tags", tags);
    }

    pub fn set_dependencies(&mut self, deps: &[String]) {
        self.set_list("dependencies", deps);
    }

    pub fn add_pr(&mut self, url: &str) {
        let mut prs = self.pr();
        if !prs.iter().any(|p| p == url) {
            prs.push(url.to_string());
        }
        self.set_list("pr", &prs);
    }

    /// Replace the markdown body, normalizing to "blank line, content, single
    /// trailing newline" so files stay tidy regardless of caller whitespace.
    pub fn set_body(&mut self, body: &str) {
        let trimmed = body.trim();
        self.body = if trimmed.is_empty() {
            "\n".to_string()
        } else {
            format!("\n{trimmed}\n")
        };
    }

    /// All frontmatter keys in order (for validate and the web UI).
    pub fn keys(&self) -> Vec<String> {
        self.doc.keys().filter_map(value_str).collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn day(y: i32, m: u32, d: u32) -> NaiveDate {
        NaiveDate::from_ymd_opt(y, m, d).unwrap()
    }

    const FULL: &str = r#"---
id: "015"
title: "Implement user authentication"
status: in-progress
priority: high
effort: large
type: feature
phase: "v1.0"
dependencies: ["012", "013"]
parent: "012"
tags:
  - auth
  - security
owner: patrick
group: web
touches:
  - cli/graph
context:
  - "docs/api-design.md"
pr: ["https://example.com/pull/42"]
external_id: "PROJ-123"
created_at: 2026-02-08
custom_field: keep me
recurring: "checkin"
verify:
  - type: bash
    run: "cargo test"
    dir: "."
  - type: assert
    check: "Tokens expire"
  - type: mystery
    what: ever
---

# Implement User Authentication

Body text.
"#;

    #[test]
    fn parses_every_field() {
        let t = Task::parse_required(FULL).unwrap();
        assert_eq!(t.id(), "015");
        assert_eq!(t.title(), "Implement user authentication");
        assert_eq!(t.status(), Some(Status::InProgress));
        assert_eq!(t.priority(), Some(Priority::High));
        assert_eq!(t.effort(), Some(Effort::Large));
        assert_eq!(t.task_type(), Some(TaskType::Feature));
        assert_eq!(t.phase().as_deref(), Some("v1.0"));
        assert_eq!(t.dependencies(), vec!["012", "013"]);
        assert_eq!(t.parent().as_deref(), Some("012"));
        assert_eq!(t.tags(), vec!["auth", "security"]);
        assert_eq!(t.owner().as_deref(), Some("patrick"));
        assert_eq!(t.group().as_deref(), Some("web"));
        assert_eq!(t.touches(), vec!["cli/graph"]);
        assert_eq!(t.context(), vec!["docs/api-design.md"]);
        assert_eq!(t.pr(), vec!["https://example.com/pull/42"]);
        assert_eq!(t.external_id().as_deref(), Some("PROJ-123"));
        assert_eq!(t.created_at(), Some(day(2026, 2, 8)));
        assert_eq!(t.recurring().as_deref(), Some("checkin"));
        assert!(t.body.contains("# Implement User Authentication"));
    }

    #[test]
    fn verify_checks_parse_typed_and_unknown() {
        let t = Task::parse_required(FULL).unwrap();
        assert_eq!(
            t.verify(),
            vec![
                VerifyCheck::Bash {
                    run: "cargo test".into(),
                    dir: Some(".".into()),
                },
                VerifyCheck::Assert {
                    check: "Tokens expire".into(),
                },
                VerifyCheck::Unknown {
                    kind: Some("mystery".into()),
                },
            ]
        );
    }

    #[test]
    fn verify_missing_or_malformed_entries() {
        let raw = "---\nid: \"1\"\ntitle: t\nverify:\n  - type: bash\n  - just-a-string\n---\n";
        let t = Task::parse_required(raw).unwrap();
        // bash without run and a non-mapping entry both degrade to Unknown.
        assert_eq!(
            t.verify(),
            vec![
                VerifyCheck::Unknown {
                    kind: Some("bash".into())
                },
                VerifyCheck::Unknown { kind: None },
            ]
        );
        // No verify key at all: empty.
        let t2 = Task::parse_required("---\nid: \"1\"\ntitle: t\n---\n").unwrap();
        assert!(t2.verify().is_empty());
    }

    #[test]
    fn round_trip_preserves_unknown_fields_and_order() {
        let t = Task::parse_required(FULL).unwrap();
        let out = t.to_markdown();
        let t2 = Task::parse_required(&out).unwrap();
        assert_eq!(t.keys(), t2.keys());
        assert_eq!(t2.get_str("custom_field").as_deref(), Some("keep me"));
        assert_eq!(t2.recurring().as_deref(), Some("checkin"));
        // Body is byte-preserved.
        assert_eq!(t.body, t2.body);
        // A second round trip is byte-identical (stable formatting).
        assert_eq!(out, t2.to_markdown());
    }

    #[test]
    fn round_trip_keeps_numeric_looking_id_a_string() {
        let t = Task::parse_required("---\nid: \"001\"\ntitle: t\n---\n\nx\n").unwrap();
        let out = t.to_markdown();
        let t2 = Task::parse_required(&out).unwrap();
        assert_eq!(t2.id(), "001");
    }

    #[test]
    fn unquoted_numeric_id_reads_as_string() {
        let t = Task::parse_required("---\nid: 42\ntitle: t\n---\n").unwrap();
        assert_eq!(t.id(), "42");
    }

    #[test]
    fn crlf_content_parses_and_fields_survive() {
        let raw = "---\r\nid: \"007\"\r\ntitle: \"T\"\r\nstatus: completed\r\nrecurring: \"k\"\r\ncompleted_at: 2026-06-13\r\n---\r\n\r\n# T\r\n";
        let t = Task::parse_required(raw).unwrap();
        assert_eq!(t.id(), "007");
        assert_eq!(t.status(), Some(Status::Completed));
        assert_eq!(t.recurring().as_deref(), Some("k"));
        assert_eq!(t.completed_at(), Some(day(2026, 6, 13)));
        // CRLF body is preserved as-is.
        assert!(t.body.contains("# T\r\n"));
    }

    #[test]
    fn created_alias_is_accepted() {
        let t =
            Task::parse_required("---\nid: \"1\"\ntitle: t\ncreated: 2026-01-02\n---\n").unwrap();
        assert_eq!(t.created_at(), Some(day(2026, 1, 2)));
        // created_at wins over the alias when both exist.
        let t2 = Task::parse_required(
            "---\nid: \"1\"\ntitle: t\ncreated_at: 2026-03-04\ncreated: 2026-01-02\n---\n",
        )
        .unwrap();
        assert_eq!(t2.created_at(), Some(day(2026, 3, 4)));
    }

    #[test]
    fn no_frontmatter_is_not_a_task() {
        assert_eq!(
            Task::parse("# taskmd Specification\n"),
            ParseOutcome::NotATask
        );
        assert_eq!(Task::parse(""), ParseOutcome::NotATask);
    }

    #[test]
    fn fenced_yaml_example_in_body_is_not_a_task() {
        // A doc that *contains* a fenced yaml task example but has no leading
        // frontmatter must not scan as a task.
        let doc = "# Spec\n\n```yaml\n---\nid: \"001\"\ntitle: \"X\"\n---\n```\n";
        assert_eq!(Task::parse(doc), ParseOutcome::NotATask);
    }

    #[test]
    fn unrelated_frontmatter_is_not_a_task() {
        // Jekyll-style frontmatter without task keys: not ours.
        let doc = "---\nlayout: post\ndate: 2026-01-01\n---\n\nHello\n";
        assert_eq!(Task::parse(doc), ParseOutcome::NotATask);
    }

    #[test]
    fn frontmatter_rulers_are_not_a_task() {
        // `---` used as a horizontal rule with prose between: the "frontmatter"
        // parses as a plain string, not a mapping.
        let doc = "---\njust prose\n---\n";
        assert_eq!(Task::parse(doc), ParseOutcome::NotATask);
    }

    #[test]
    fn unterminated_frontmatter_is_not_a_task() {
        let doc = "---\nid: \"1\"\ntitle: t\nno closing fence\n";
        assert_eq!(Task::parse(doc), ParseOutcome::NotATask);
    }

    #[test]
    fn missing_id_or_title_is_invalid() {
        let no_id = "---\ntitle: \"X\"\nstatus: pending\n---\n";
        assert!(matches!(
            Task::parse(no_id),
            ParseOutcome::Invalid(r) if r.contains("`id`")
        ));
        let no_title = "---\nid: \"001\"\nstatus: pending\n---\n";
        assert!(matches!(
            Task::parse(no_title),
            ParseOutcome::Invalid(r) if r.contains("`title`")
        ));
        let empty_id = "---\nid: \"\"\ntitle: \"X\"\n---\n";
        assert!(matches!(Task::parse(empty_id), ParseOutcome::Invalid(_)));
    }

    #[test]
    fn malformed_yaml_with_fences_is_invalid() {
        let doc = "---\nid: \"1\"\ntitle: [unclosed\n---\n";
        assert!(matches!(
            Task::parse(doc),
            ParseOutcome::Invalid(r) if r.contains("malformed frontmatter")
        ));
    }

    #[test]
    fn parse_required_errors() {
        assert!(Task::parse_required("no fm").is_err());
        assert!(Task::parse_required("---\ntitle: only\n---\n").is_err());
        assert!(Task::parse_required(FULL).is_ok());
    }

    #[test]
    fn new_task_has_canonical_frontmatter() {
        let t = Task::new("009", "Do the thing", day(2026, 7, 2));
        assert_eq!(t.id(), "009");
        assert_eq!(t.title(), "Do the thing");
        assert_eq!(t.effective_status(), Status::Pending);
        assert_eq!(t.created_at(), Some(day(2026, 7, 2)));
        let out = t.to_markdown();
        assert!(out.starts_with("---\n"));
        // The numeric-looking id must be quoted so it stays a string.
        assert!(out.contains("id: '009'"), "got:\n{out}");
        assert!(out.contains("# Do the thing"));
        // And it must re-parse as a task.
        assert!(matches!(Task::parse(&out), ParseOutcome::Task(_)));
    }

    #[test]
    fn effective_defaults() {
        let t = Task::parse_required("---\nid: \"1\"\ntitle: t\n---\n").unwrap();
        assert_eq!(t.status(), None);
        assert_eq!(t.effective_status(), Status::Pending);
        assert_eq!(t.priority(), None);
        assert_eq!(t.effective_priority(), Priority::Medium);
        assert_eq!(t.effort(), None);
        assert_eq!(t.task_type(), None);
        assert!(t.dependencies().is_empty());
        assert!(t.tags().is_empty());
        assert_eq!(t.group(), None);
        assert_eq!(t.owner(), None);
        assert_eq!(t.parent(), None);
        assert_eq!(t.created_at(), None);
    }

    #[test]
    fn invalid_enum_values_read_as_none_but_raw_survives() {
        let t = Task::parse_required(
            "---\nid: \"1\"\ntitle: t\nstatus: done\npriority: urgent\neffort: epic\ntype: story\n---\n",
        )
        .unwrap();
        assert_eq!(t.status(), None);
        assert_eq!(t.status_raw().as_deref(), Some("done"));
        assert_eq!(t.priority(), None);
        assert_eq!(t.priority_raw().as_deref(), Some("urgent"));
        assert_eq!(t.effort(), None);
        assert_eq!(t.effort_raw().as_deref(), Some("epic"));
        assert_eq!(t.task_type(), None);
        assert_eq!(t.task_type_raw().as_deref(), Some("story"));
    }

    #[test]
    fn scalar_where_list_expected_reads_as_one_entry() {
        let t = Task::parse_required("---\nid: \"1\"\ntitle: t\ntags: solo\n---\n").unwrap();
        assert_eq!(t.tags(), vec!["solo"]);
    }

    #[test]
    fn status_transitions_maintain_timestamps() {
        let mut t = Task::new("1", "t", day(2026, 7, 1));
        t.set_status(Status::Completed, day(2026, 7, 2));
        assert_eq!(t.completed_at(), Some(day(2026, 7, 2)));
        assert_eq!(t.cancelled_at(), None);

        // Idempotent re-complete keeps the original date.
        t.set_status(Status::Completed, day(2026, 7, 9));
        assert_eq!(t.completed_at(), Some(day(2026, 7, 2)));

        // Reopening clears completed_at.
        t.set_status(Status::Pending, day(2026, 7, 3));
        assert_eq!(t.status(), Some(Status::Pending));
        assert_eq!(t.completed_at(), None);

        // Cancelling stamps cancelled_at.
        t.set_status(Status::Cancelled, day(2026, 7, 4));
        assert_eq!(t.cancelled_at(), Some(day(2026, 7, 4)));
        assert_eq!(t.completed_at(), None);
        t.set_status(Status::Cancelled, day(2026, 7, 8));
        assert_eq!(t.cancelled_at(), Some(day(2026, 7, 4)));

        // Cancelled -> completed swaps the timestamps.
        t.set_status(Status::Completed, day(2026, 7, 5));
        assert_eq!(t.completed_at(), Some(day(2026, 7, 5)));
        assert_eq!(t.cancelled_at(), None);

        // Terminal -> in-progress clears everything.
        t.set_status(Status::InProgress, day(2026, 7, 6));
        assert_eq!(t.completed_at(), None);
        assert_eq!(t.cancelled_at(), None);
    }

    #[test]
    fn typed_mutators_write_through() {
        let mut t = Task::new("1", "old", day(2026, 7, 1));
        t.set_title("new title");
        t.set_priority(Priority::Critical);
        t.set_effort(Effort::Small);
        t.set_task_type(TaskType::Bug);
        t.set_phase(Some("v1"));
        t.set_owner(Some("patrick"));
        t.set_parent(Some("045"));
        t.set_tags(&["a".into(), "b".into()]);
        t.set_dependencies(&["002".into()]);
        t.add_pr("https://x/1");
        t.add_pr("https://x/1"); // dedup
        t.add_pr("https://x/2");
        let out = t.to_markdown();
        let t2 = Task::parse_required(&out).unwrap();
        assert_eq!(t2.title(), "new title");
        assert_eq!(t2.priority(), Some(Priority::Critical));
        assert_eq!(t2.effort(), Some(Effort::Small));
        assert_eq!(t2.task_type(), Some(TaskType::Bug));
        assert_eq!(t2.phase().as_deref(), Some("v1"));
        assert_eq!(t2.owner().as_deref(), Some("patrick"));
        assert_eq!(t2.parent().as_deref(), Some("045"));
        assert_eq!(t2.tags(), vec!["a", "b"]);
        assert_eq!(t2.dependencies(), vec!["002"]);
        assert_eq!(t2.pr(), vec!["https://x/1", "https://x/2"]);

        // Clearing optionals removes the keys entirely.
        t.set_phase(None);
        t.set_owner(None);
        t.set_parent(None);
        let out = t.to_markdown();
        assert!(!out.contains("phase:"));
        assert!(!out.contains("owner:"));
        assert!(!out.contains("parent:"));
    }

    #[test]
    fn due_accessors_and_setter_round_trip() {
        let mut t = Task::new("1", "t", day(2026, 7, 1));
        assert_eq!(t.due_raw(), None);
        assert_eq!(t.due(), None);
        t.set_due(Some("2026-08-01"));
        let t2 = Task::parse_required(&t.to_markdown()).unwrap();
        assert_eq!(t2.due_raw().as_deref(), Some("2026-08-01"));
        assert_eq!(t2.due(), Some(day(2026, 8, 1)));
        // A malformed value survives verbatim but does not parse.
        t.set_due(Some("not-a-date"));
        assert_eq!(t.due_raw().as_deref(), Some("not-a-date"));
        assert_eq!(t.due(), None);
        // Clearing removes the key.
        t.set_due(None);
        assert!(!t.to_markdown().contains("due:"));
    }

    #[test]
    fn set_body_normalizes_whitespace() {
        let mut t = Task::new("1", "t", day(2026, 7, 1));
        t.set_body("\n\n# Heading\n\ncontent\n\n\n");
        assert_eq!(t.body, "\n# Heading\n\ncontent\n");
        t.set_body("   ");
        assert_eq!(t.body, "\n");
        assert!(t.to_markdown().ends_with("---\n\n"));
    }

    #[test]
    fn enum_string_round_trips() {
        for s in Status::ALL {
            assert_eq!(Status::parse(s.as_str()), Some(s));
        }
        for p in Priority::ALL {
            assert_eq!(Priority::parse(p.as_str()), Some(p));
        }
        for e in Effort::ALL {
            assert_eq!(Effort::parse(e.as_str()), Some(e));
        }
        for t in TaskType::ALL {
            assert_eq!(TaskType::parse(t.as_str()), Some(t));
        }
        assert_eq!(Status::parse("done"), None);
    }

    #[test]
    fn enum_ordering_for_comparisons() {
        assert!(Priority::Critical > Priority::High);
        assert!(Priority::High > Priority::Medium);
        assert!(Priority::Medium > Priority::Low);
        assert!(Effort::Large > Effort::Medium);
        assert!(Effort::Medium > Effort::Small);
    }

    #[test]
    fn status_terminality() {
        assert!(Status::Completed.is_terminal());
        assert!(Status::Cancelled.is_terminal());
        assert!(!Status::Pending.is_terminal());
        assert!(!Status::InProgress.is_terminal());
        assert!(!Status::InReview.is_terminal());
        assert!(!Status::Blocked.is_terminal());
    }

    #[test]
    fn split_frontmatter_edge_cases() {
        // Fence-like line that is not exactly `---` does not close.
        assert!(split_frontmatter("---\na: 1\n----\n").is_none());
        // Closing fence at EOF without trailing newline.
        let (fm, body) = split_frontmatter("---\na: 1\n---").unwrap();
        assert_eq!(fm, "a: 1\n");
        assert_eq!(body, "");
        // CRLF closing fence.
        let (fm, body) = split_frontmatter("---\r\na: 1\r\n---\r\nbody").unwrap();
        assert_eq!(fm, "a: 1\r\n");
        assert_eq!(body, "body");
    }

    #[test]
    fn value_str_coercions() {
        assert_eq!(value_str(&Value::Bool(true)).as_deref(), Some("true"));
        assert_eq!(value_str(&Value::Null), None);
        let t = Task::parse_required("---\nid: \"1\"\ntitle: t\ntags: [1, 2]\n---\n").unwrap();
        assert_eq!(t.tags(), vec!["1", "2"]);
    }

    #[test]
    fn get_and_keys_expose_the_doc() {
        let t = Task::parse_required("---\nid: \"1\"\ntitle: t\nweird: [1]\n---\n").unwrap();
        assert!(t.get("weird").is_some());
        assert!(t.get("absent").is_none());
        assert_eq!(t.keys(), vec!["id", "title", "weird"]);
    }
}
