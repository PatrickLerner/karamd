//! Query mini-grammar over tasks.
//!
//! ```text
//! expr    := or
//! or      := and ( OR and )*
//! and     := unary ( AND unary )*
//! unary   := NOT unary | "(" expr ")" | term
//! term    := field op value
//! op      := ":" | ">=" | ">" | "<=" | "<"
//! value   := bare-word | "quoted string"
//! ```
//!
//! Fields: `status`, `priority`, `effort`, `type`, `phase`, `tag`, `owner`,
//! `group`, `scope` (matches `touches`), `id`, `title` (substring,
//! case-insensitive), `depends` (has that id as dependency), `ready`
//! (true/false, from the dependency graph), `open` (true/false: status is not
//! terminal, i.e. neither `completed` nor `cancelled`), and the dates
//! `created`, `completed`, `cancelled`. Ordering operators work on `priority`, `effort`
//! (their enum order) and the date fields; everything else is `:` only.
//! `AND`/`OR`/`NOT` are case-insensitive; a hand-rolled recursive-descent
//! parser keeps the dependency set at zero.

use anyhow::{Result, bail};
use chrono::NaiveDate;

use crate::taskmd::{Effort, Priority, Status, Task, TaskType};

/// A parsed query, ready to evaluate against tasks.
#[derive(Debug, Clone, PartialEq)]
pub enum Expr {
    And(Box<Expr>, Box<Expr>),
    Or(Box<Expr>, Box<Expr>),
    Not(Box<Expr>),
    Term(Term),
}

#[derive(Debug, Clone, PartialEq)]
pub struct Term {
    pub field: Field,
    pub op: Op,
    pub value: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Field {
    Status,
    Priority,
    Effort,
    Type,
    Phase,
    Tag,
    Owner,
    Group,
    Scope,
    Id,
    Title,
    Depends,
    Ready,
    Open,
    Created,
    Completed,
    Cancelled,
}

impl Field {
    fn parse(s: &str) -> Option<Field> {
        Some(match s {
            "status" => Field::Status,
            "priority" => Field::Priority,
            "effort" => Field::Effort,
            "type" => Field::Type,
            "phase" => Field::Phase,
            "tag" | "tags" => Field::Tag,
            "owner" => Field::Owner,
            "group" => Field::Group,
            "scope" | "touches" => Field::Scope,
            "id" => Field::Id,
            "title" => Field::Title,
            "depends" => Field::Depends,
            "ready" => Field::Ready,
            "open" => Field::Open,
            "created" | "created_at" => Field::Created,
            "completed" | "completed_at" => Field::Completed,
            "cancelled" | "cancelled_at" => Field::Cancelled,
            _ => return None,
        })
    }

    fn is_ordered(self) -> bool {
        matches!(
            self,
            Field::Priority | Field::Effort | Field::Created | Field::Completed | Field::Cancelled
        )
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Op {
    Eq,
    Ge,
    Gt,
    Le,
    Lt,
}

#[derive(Debug, PartialEq)]
enum Token {
    Open,
    Close,
    And,
    Or,
    Not,
    Term(Term),
}

fn lex(input: &str) -> Result<Vec<Token>> {
    let mut tokens = Vec::new();
    let mut chars = input.chars().peekable();
    while let Some(&c) = chars.peek() {
        match c {
            c if c.is_whitespace() => {
                chars.next();
            }
            '(' => {
                chars.next();
                tokens.push(Token::Open);
            }
            ')' => {
                chars.next();
                tokens.push(Token::Close);
            }
            _ => {
                let mut word = String::new();
                let mut in_quotes = false;
                while let Some(&c) = chars.peek() {
                    if c == '"' {
                        in_quotes = !in_quotes;
                        word.push(c);
                        chars.next();
                    } else if !in_quotes && (c.is_whitespace() || c == '(' || c == ')') {
                        break;
                    } else {
                        word.push(c);
                        chars.next();
                    }
                }
                if in_quotes {
                    bail!("unterminated quote in query near `{word}`");
                }
                tokens.push(classify(&word)?);
            }
        }
    }
    Ok(tokens)
}

fn classify(word: &str) -> Result<Token> {
    if word.eq_ignore_ascii_case("and") {
        return Ok(Token::And);
    }
    if word.eq_ignore_ascii_case("or") {
        return Ok(Token::Or);
    }
    if word.eq_ignore_ascii_case("not") {
        return Ok(Token::Not);
    }
    // Longest operators first so `>=` is not read as `>` + `=`.
    for (op_str, op) in [
        (">=", Op::Ge),
        ("<=", Op::Le),
        (">", Op::Gt),
        ("<", Op::Lt),
        (":", Op::Eq),
    ] {
        if let Some(pos) = word.find(op_str) {
            let (field_str, rest) = word.split_at(pos);
            let value = rest[op_str.len()..].trim_matches('"').to_string();
            let Some(field) = Field::parse(field_str) else {
                bail!(
                    "unknown field `{field_str}` (fields: status, priority, effort, type, phase, \
                     tag, owner, group, scope, id, title, depends, ready, open, created, \
                     completed, cancelled)"
                );
            };
            if value.is_empty() {
                bail!("empty value in query term `{word}`");
            }
            if op != Op::Eq && !field.is_ordered() {
                bail!(
                    "`{field_str}` only supports `:` (ordering works on priority, effort, dates)"
                );
            }
            validate_value(field, &value)?;
            return Ok(Token::Term(Term { field, op, value }));
        }
    }
    bail!("expected `field:value` (or AND/OR/NOT), got `{word}`")
}

/// Enum-valued and date-valued fields reject junk at parse time, so a typo
/// fails loudly instead of matching nothing.
fn validate_value(field: Field, value: &str) -> Result<()> {
    match field {
        Field::Status if Status::parse(value).is_none() => {
            bail!(
                "invalid status `{value}` (pending, in-progress, in-review, completed, blocked, cancelled)"
            )
        }
        Field::Priority if Priority::parse(value).is_none() => {
            bail!("invalid priority `{value}` (low, medium, high, critical)")
        }
        Field::Effort if Effort::parse(value).is_none() => {
            bail!("invalid effort `{value}` (small, medium, large)")
        }
        Field::Type if TaskType::parse(value).is_none() => {
            bail!("invalid type `{value}` (feature, bug, improvement, chore, docs)")
        }
        Field::Ready if !matches!(value, "true" | "false") => {
            bail!("`ready` takes true or false")
        }
        Field::Open if !matches!(value, "true" | "false") => {
            bail!("`open` takes true or false")
        }
        Field::Created | Field::Completed | Field::Cancelled
            if NaiveDate::parse_from_str(value, "%Y-%m-%d").is_err() =>
        {
            bail!("invalid date `{value}` (need YYYY-MM-DD)")
        }
        _ => Ok(()),
    }
}

/// Parse a query string into an [`Expr`]. An all-whitespace query is an error;
/// callers treat "no query" as "match everything" themselves.
pub fn parse(input: &str) -> Result<Expr> {
    let tokens = lex(input)?;
    if tokens.is_empty() {
        bail!("empty query");
    }
    let mut pos = 0;
    let expr = parse_or(&tokens, &mut pos)?;
    if pos != tokens.len() {
        bail!("unexpected trailing input in query");
    }
    Ok(expr)
}

fn parse_or(tokens: &[Token], pos: &mut usize) -> Result<Expr> {
    let mut left = parse_and(tokens, pos)?;
    while tokens.get(*pos) == Some(&Token::Or) {
        *pos += 1;
        let right = parse_and(tokens, pos)?;
        left = Expr::Or(Box::new(left), Box::new(right));
    }
    Ok(left)
}

fn parse_and(tokens: &[Token], pos: &mut usize) -> Result<Expr> {
    let mut left = parse_unary(tokens, pos)?;
    while tokens.get(*pos) == Some(&Token::And) {
        *pos += 1;
        let right = parse_unary(tokens, pos)?;
        left = Expr::And(Box::new(left), Box::new(right));
    }
    Ok(left)
}

fn parse_unary(tokens: &[Token], pos: &mut usize) -> Result<Expr> {
    match tokens.get(*pos) {
        Some(Token::Not) => {
            *pos += 1;
            Ok(Expr::Not(Box::new(parse_unary(tokens, pos)?)))
        }
        Some(Token::Open) => {
            *pos += 1;
            let inner = parse_or(tokens, pos)?;
            if tokens.get(*pos) != Some(&Token::Close) {
                bail!("missing closing `)` in query");
            }
            *pos += 1;
            Ok(inner)
        }
        Some(Token::Term(t)) => {
            *pos += 1;
            Ok(Expr::Term(t.clone()))
        }
        _ => bail!("expected a term, NOT, or `(` in query"),
    }
}

/// Per-task context the evaluator needs beyond the task itself.
pub struct EvalCtx {
    /// From the dependency graph: all dependencies `completed`.
    pub ready: bool,
}

/// Evaluate a parsed query against one task.
pub fn eval(expr: &Expr, task: &Task, ctx: &EvalCtx) -> bool {
    match expr {
        Expr::And(a, b) => eval(a, task, ctx) && eval(b, task, ctx),
        Expr::Or(a, b) => eval(a, task, ctx) || eval(b, task, ctx),
        Expr::Not(inner) => !eval(inner, task, ctx),
        Expr::Term(t) => eval_term(t, task, ctx),
    }
}

fn cmp_matches<T: Ord>(op: Op, left: T, right: T) -> bool {
    match op {
        Op::Eq => left == right,
        Op::Ge => left >= right,
        Op::Gt => left > right,
        Op::Le => left <= right,
        Op::Lt => left < right,
    }
}

fn eval_term(term: &Term, task: &Task, ctx: &EvalCtx) -> bool {
    let v = term.value.as_str();
    match term.field {
        Field::Status => task.effective_status().as_str() == v,
        // Values were validated at parse time, so `expect` cannot fire.
        Field::Priority => cmp_matches(
            term.op,
            task.effective_priority(),
            Priority::parse(v).expect("validated"),
        ),
        // No spec default for effort: a task without one matches nothing.
        Field::Effort => task
            .effort()
            .is_some_and(|e| cmp_matches(term.op, e, Effort::parse(v).expect("validated"))),
        Field::Type => task.task_type().is_some_and(|t| t.as_str() == v),
        Field::Phase => task.phase().as_deref() == Some(v),
        Field::Tag => task.tags().iter().any(|t| t == v),
        Field::Owner => task.owner().as_deref() == Some(v),
        Field::Group => task.group().as_deref() == Some(v),
        Field::Scope => task.touches().iter().any(|t| t == v),
        Field::Id => task.id() == v,
        Field::Title => task.title().to_lowercase().contains(&v.to_lowercase()),
        Field::Depends => task.dependencies().iter().any(|d| d == v),
        Field::Ready => ctx.ready == (v == "true"),
        // open == !terminal; `terminal != wanted` is the de-negated form of
        // `(!terminal) == wanted`.
        Field::Open => task.effective_status().is_terminal() != (v == "true"),
        Field::Created => date_matches(term.op, task.created_at(), v),
        Field::Completed => date_matches(term.op, task.completed_at(), v),
        Field::Cancelled => date_matches(term.op, task.cancelled_at(), v),
    }
}

fn date_matches(op: Op, actual: Option<NaiveDate>, value: &str) -> bool {
    let wanted = NaiveDate::parse_from_str(value, "%Y-%m-%d").expect("validated");
    actual.is_some_and(|d| cmp_matches(op, d, wanted))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn task(yaml: &str) -> Task {
        Task::parse_required(&format!("---\n{yaml}\n---\n")).unwrap()
    }

    fn matches(query: &str, yaml: &str) -> bool {
        matches_ctx(query, yaml, true)
    }

    fn matches_ctx(query: &str, yaml: &str, ready: bool) -> bool {
        let expr = parse(query).unwrap();
        eval(&expr, &task(yaml), &EvalCtx { ready })
    }

    const T: &str = r#"id: "015"
title: "Implement user auth"
status: in-progress
priority: high
effort: large
type: feature
phase: v1
tags: [auth, api]
touches: [cli/graph]
dependencies: ["012"]
owner: patrick
created_at: 2026-02-08
"#;

    #[test]
    fn simple_field_terms() {
        assert!(matches("status:in-progress", T));
        assert!(!matches("status:pending", T));
        assert!(matches("priority:high", T));
        assert!(matches("effort:large", T));
        assert!(matches("type:feature", T));
        assert!(matches("phase:v1", T));
        assert!(matches("tag:auth", T));
        assert!(matches("tags:api", T));
        assert!(!matches("tag:web", T));
        assert!(matches("owner:patrick", T));
        assert!(matches("scope:cli/graph", T));
        assert!(matches("touches:cli/graph", T));
        assert!(matches("id:015", T));
        assert!(matches("depends:012", T));
        assert!(!matches("depends:404", T));
    }

    #[test]
    fn title_is_case_insensitive_substring() {
        assert!(matches("title:USER", T));
        assert!(matches("title:\"user auth\"", T));
        assert!(!matches("title:database", T));
    }

    #[test]
    fn priority_and_effort_comparisons() {
        assert!(matches("priority>=high", T));
        assert!(matches("priority>=medium", T));
        assert!(!matches("priority>=critical", T));
        assert!(matches("priority>medium", T));
        assert!(!matches("priority>high", T));
        assert!(matches("priority<=high", T));
        assert!(!matches("priority<high", T));
        assert!(matches("effort>medium", T));
        assert!(matches("effort>=large", T));
        assert!(!matches("effort<large", T));
    }

    #[test]
    fn missing_priority_defaults_to_medium_but_missing_effort_never_matches() {
        let bare = "id: \"1\"\ntitle: t";
        assert!(matches("priority:medium", bare));
        assert!(matches("priority>=medium", bare));
        assert!(!matches("priority>=high", bare));
        assert!(!matches("effort:small", bare));
        assert!(!matches("effort<=large", bare));
        assert!(!matches("type:bug", bare));
        assert!(!matches("owner:patrick", bare));
        assert!(!matches("phase:v1", bare));
    }

    #[test]
    fn status_defaults_to_pending() {
        assert!(matches("status:pending", "id: \"1\"\ntitle: t"));
    }

    #[test]
    fn ready_uses_graph_context() {
        assert!(matches_ctx("ready:true", T, true));
        assert!(!matches_ctx("ready:true", T, false));
        assert!(matches_ctx("ready:false", T, false));
    }

    #[test]
    fn open_matches_non_terminal_status() {
        // T is in-progress: open.
        assert!(matches("open:true", T));
        assert!(!matches("open:false", T));
        // Missing status defaults to pending: open.
        let bare = "id: \"1\"\ntitle: t";
        assert!(matches("open:true", bare));
        // Terminal states are not open.
        let done = "id: \"1\"\ntitle: t\nstatus: completed";
        assert!(!matches("open:true", done));
        assert!(matches("open:false", done));
        let gone = "id: \"1\"\ntitle: t\nstatus: cancelled";
        assert!(!matches("open:true", gone));
        assert!(matches("open:false", gone));
        // Composes with other terms.
        assert!(matches("open:true AND tag:auth", T));
    }

    #[test]
    fn date_comparisons() {
        assert!(matches("created:2026-02-08", T));
        assert!(matches("created>=2026-01-01", T));
        assert!(matches("created<2026-03-01", T));
        assert!(!matches("created>2026-02-08", T));
        assert!(matches("created_at:2026-02-08", T)); // alias
        // Task without the date never matches.
        assert!(!matches("completed>=2020-01-01", T));
        assert!(!matches("cancelled:2026-01-01", T));
    }

    #[test]
    fn completed_and_cancelled_dates_match_when_present() {
        let done = "id: \"1\"\ntitle: t\nstatus: completed\ncompleted_at: 2026-06-30";
        assert!(matches("completed>=2026-06-01", done));
        let gone = "id: \"1\"\ntitle: t\nstatus: cancelled\ncancelled_at: 2026-06-30";
        assert!(matches("cancelled:2026-06-30", gone));
    }

    #[test]
    fn boolean_combinators_and_precedence() {
        // AND binds tighter than OR.
        assert!(matches("status:in-progress AND priority>=high", T));
        assert!(!matches("status:pending AND priority>=high", T));
        assert!(matches("status:pending OR priority>=high", T));
        assert!(matches(
            "status:pending OR status:in-progress AND tag:auth",
            T
        ));
        // Parentheses override.
        assert!(!matches(
            "(status:pending OR status:blocked) AND tag:auth",
            T
        ));
        assert!(matches(
            "(status:pending OR status:in-progress) AND tag:auth",
            T
        ));
        // NOT.
        assert!(matches("NOT status:pending", T));
        assert!(!matches("NOT tag:auth", T));
        assert!(matches("NOT (status:pending AND tag:auth)", T));
        // Case-insensitive keywords.
        assert!(matches("not status:pending and tag:auth or id:404", T));
    }

    #[test]
    fn group_field_matches_explicit_group() {
        assert!(matches("group:web", "id: \"1\"\ntitle: t\ngroup: web"));
        assert!(!matches("group:web", "id: \"1\"\ntitle: t"));
    }

    #[test]
    fn parse_errors_are_loud_and_specific() {
        for (query, needle) in [
            ("", "empty query"),
            ("   ", "empty query"),
            ("bogus:x", "unknown field `bogus`"),
            ("status:", "empty value"),
            ("pending", "expected `field:value`"),
            ("status:done", "invalid status `done`"),
            ("priority:urgent", "invalid priority"),
            ("effort:epic", "invalid effort"),
            ("type:story", "invalid type"),
            ("ready:maybe", "`ready` takes true or false"),
            ("open:maybe", "`open` takes true or false"),
            ("created:tomorrow", "invalid date"),
            ("title>=x", "only supports `:`"),
            ("status>pending", "only supports `:`"),
            ("(status:pending", "missing closing"),
            ("status:pending)", "unexpected trailing"),
            ("status:pending AND", "expected a term"),
            ("AND status:pending", "expected a term"),
            ("title:\"unterminated", "unterminated quote"),
        ] {
            let err = parse(query).unwrap_err().to_string();
            assert!(err.contains(needle), "query `{query}`: got `{err}`");
        }
    }

    #[test]
    fn quoted_values_can_contain_spaces_and_parens() {
        assert!(matches("title:\"user auth\"", T));
        let t = "id: \"1\"\ntitle: \"weird (thing)\"";
        assert!(matches("title:\"(thing)\"", t));
    }

    #[test]
    fn nested_parentheses() {
        assert!(matches(
            "((status:in-progress OR status:pending) AND (tag:auth OR tag:web))",
            T
        ));
    }

    #[test]
    fn double_not_cancels() {
        assert!(matches("NOT NOT tag:auth", T));
    }
}
