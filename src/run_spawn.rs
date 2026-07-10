//! Real subprocess [`AgentRunner`] for `karamd run` (#039).
//!
//! This is the process + timeout glue, excluded from coverage like
//! [`crate::web_terminal`]: it only spawns a child, waits with a deadline, tees
//! its output to the per-run log (#045), and maps the result to an
//! [`AgentOutcome`]. All the decision logic it relies on (prompt substitution)
//! is the covered [`crate::run::substitute_prompt`].

use std::fs::File;
use std::io::{Read, Write};
use std::path::Path;
use std::process::{Child, Command, Stdio};
use std::sync::{Arc, Mutex};
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
        exit_code: None,
        duration_s: 0,
    }
}

/// Pump a child pipe to both the console stream and (if any) the shared log
/// file, so the operator still sees live output while it is also persisted.
fn tee<R: Read + Send + 'static, W: Write + Send + 'static>(
    mut reader: R,
    mut console: W,
    log: Option<Arc<Mutex<File>>>,
) -> thread::JoinHandle<()> {
    thread::spawn(move || {
        let mut buf = [0u8; 8192];
        loop {
            match reader.read(&mut buf) {
                Ok(0) | Err(_) => break,
                Ok(n) => {
                    let _ = console.write_all(&buf[..n]);
                    let _ = console.flush();
                    if let Some(log) = &log
                        && let Ok(mut f) = log.lock()
                    {
                        let _ = f.write_all(&buf[..n]);
                    }
                }
            }
        }
    })
}

impl AgentRunner for ProcessRunner {
    fn run(
        &self,
        spec: &AgentSpec,
        prompt: &str,
        working_dir: &Path,
        timeout_secs: u64,
        log_path: Option<&Path>,
    ) -> AgentOutcome {
        if spec.command.is_empty() {
            return fail("agent command is empty");
        }

        // Open the per-run log for the tee. Logging is best-effort: if the file
        // can't be created, warn and fall back to inherited output rather than
        // failing the agent (which would burn an attempt for a logging problem).
        let log_file = match log_path {
            Some(p) => match File::create(p) {
                Ok(f) => Some(Arc::new(Mutex::new(f))),
                Err(e) => {
                    eprintln!(
                        "karamd: warning: could not open run log {}: {e}; continuing without capture",
                        p.display()
                    );
                    None
                }
            },
            None => None,
        };

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
            });
        // With a log file: capture stdout/stderr through pipes and tee them.
        // Without: inherit, as before (no capture requested).
        if log_file.is_some() {
            cmd.stdout(Stdio::piped()).stderr(Stdio::piped());
        } else {
            cmd.stdout(Stdio::inherit()).stderr(Stdio::inherit());
        }

        let start = Instant::now();
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

        // Tee threads (only when capturing). Detached, not joined: a grandchild
        // that inherits and holds the pipe open would keep the reader from ever
        // seeing EOF, so joining could hang the whole run forever. The threads
        // end on their own at EOF, or are torn down when the process exits.
        if let Some(log) = &log_file {
            if let Some(out) = child.stdout.take() {
                tee(out, std::io::stdout(), Some(log.clone()));
            }
            if let Some(err) = child.stderr.take() {
                tee(err, std::io::stderr(), Some(log.clone()));
            }
        }

        let outcome = wait_with_timeout(&mut child, start, timeout_secs);
        cleanup(prompt_file.as_deref());
        outcome
    }
}

/// Poll the child until it exits or the deadline passes, mapping the result to
/// an [`AgentOutcome`] with the exit code and elapsed wall-clock seconds.
fn wait_with_timeout(child: &mut Child, start: Instant, timeout_secs: u64) -> AgentOutcome {
    let deadline = start + Duration::from_secs(timeout_secs);
    loop {
        match child.try_wait() {
            Ok(Some(status)) => {
                let duration_s = start.elapsed().as_secs() as i64;
                break if status.success() {
                    AgentOutcome {
                        success: true,
                        detail: String::new(),
                        exit_code: status.code(),
                        duration_s,
                    }
                } else {
                    AgentOutcome {
                        success: false,
                        detail: format!("agent exited with {status}"),
                        exit_code: status.code(),
                        duration_s,
                    }
                };
            }
            Ok(None) => {
                if Instant::now() >= deadline {
                    let _ = child.kill();
                    let _ = child.wait();
                    let duration_s = start.elapsed().as_secs() as i64;
                    break AgentOutcome {
                        success: false,
                        detail: format!("timed out after {timeout_secs}s"),
                        exit_code: None,
                        duration_s,
                    };
                }
                thread::sleep(Duration::from_millis(100));
            }
            Err(e) => break fail(format!("waiting on agent: {e}")),
        }
    }
}

fn cleanup(prompt_file: Option<&Path>) {
    if let Some(path) = prompt_file {
        let _ = std::fs::remove_file(path);
    }
}
