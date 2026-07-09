//! Real subprocess [`AgentRunner`] for `karamd run` (#039).
//!
//! This is the process + timeout glue, excluded from coverage like
//! [`crate::web_terminal`]: it only spawns a child, waits with a deadline, and
//! maps the result to an [`AgentOutcome`]. All the decision logic it relies on
//! (prompt substitution) is the covered [`crate::run::substitute_prompt`].

use std::io::Write;
use std::path::Path;
use std::process::{Command, Stdio};
use std::thread;
use std::time::{Duration, Instant};

use crate::run::{AgentOutcome, AgentRunner, substitute_prompt};
use crate::taskmd::config::{AgentSpec, PromptVia};

/// Spawns agents as real OS processes.
pub struct ProcessRunner;

fn fail(detail: impl Into<String>) -> AgentOutcome {
    AgentOutcome {
        success: false,
        detail: detail.into(),
    }
}

impl AgentRunner for ProcessRunner {
    fn run(
        &self,
        spec: &AgentSpec,
        prompt: &str,
        working_dir: &Path,
        timeout_secs: u64,
    ) -> AgentOutcome {
        if spec.command.is_empty() {
            return fail("agent command is empty");
        }

        // Build argv + optional stdin per the agent's prompt-delivery mode.
        let mut prompt_file = None;
        let (args, stdin_text) = match spec.prompt_via {
            PromptVia::Arg => (substitute_prompt(&spec.command, "{prompt}", prompt), None),
            PromptVia::Stdin => (spec.command.clone(), Some(prompt.to_string())),
            PromptVia::File => {
                let path =
                    std::env::temp_dir().join(format!("karamd-prompt-{}.md", std::process::id()));
                if let Err(e) = std::fs::write(&path, prompt) {
                    return fail(format!("writing prompt file {}: {e}", path.display()));
                }
                let args =
                    substitute_prompt(&spec.command, "{prompt_file}", &path.to_string_lossy());
                prompt_file = Some(path);
                (args, None)
            }
        };

        let (program, rest) = args.split_first().expect("command checked non-empty");
        let mut cmd = Command::new(program);
        cmd.args(rest)
            .current_dir(working_dir)
            .stdin(if stdin_text.is_some() {
                Stdio::piped()
            } else {
                Stdio::null()
            })
            // Inherit stdout/stderr so the operator sees agent output in the
            // cron/terminal log; a per-run log file is a follow-up.
            .stdout(Stdio::inherit())
            .stderr(Stdio::inherit());

        let mut child = match cmd.spawn() {
            Ok(c) => c,
            Err(e) => {
                cleanup(prompt_file.as_deref());
                return fail(format!("spawning `{program}`: {e}"));
            }
        };
        if let Some(text) = stdin_text
            && let Some(mut stdin) = child.stdin.take()
        {
            let _ = stdin.write_all(text.as_bytes());
        }

        let deadline = Instant::now() + Duration::from_secs(timeout_secs);
        let result = loop {
            match child.try_wait() {
                Ok(Some(status)) => {
                    break if status.success() {
                        AgentOutcome {
                            success: true,
                            detail: String::new(),
                        }
                    } else {
                        fail(format!("agent exited with {status}"))
                    };
                }
                Ok(None) => {
                    if Instant::now() >= deadline {
                        let _ = child.kill();
                        let _ = child.wait();
                        break fail(format!("timed out after {timeout_secs}s"));
                    }
                    thread::sleep(Duration::from_millis(100));
                }
                Err(e) => break fail(format!("waiting on agent: {e}")),
            }
        };
        cleanup(prompt_file.as_deref());
        result
    }
}

fn cleanup(prompt_file: Option<&Path>) {
    if let Some(path) = prompt_file {
        let _ = std::fs::remove_file(path);
    }
}
