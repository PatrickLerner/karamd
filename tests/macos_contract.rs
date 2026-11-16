//! The JSON the macOS app decodes. The app's Swift tests read the same golden files,
//! so a change to this output fails here until it is blessed, then fails there until the
//! app handles it. Bless with `KARAMD_BLESS=1 cargo test --test macos_contract`.

use serde_json::Value;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::{SystemTime, UNIX_EPOCH};

const FIXTURES: &str = "macos/Tests/KaramdBarCoreTests/Fixtures";

fn copy_dir(from: &Path, to: &Path) {
    fs::create_dir_all(to).unwrap();
    for entry in fs::read_dir(from).unwrap() {
        let entry = entry.unwrap();
        let dest = to.join(entry.file_name());
        if entry.file_type().unwrap().is_dir() {
            copy_dir(&entry.path(), &dest);
        } else {
            fs::copy(entry.path(), dest).unwrap();
        }
    }
}

fn scratch_vault(root: &Path) -> PathBuf {
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    let dir = std::env::temp_dir().join(format!("karamd-contract-{}-{nanos}", std::process::id()));
    copy_dir(&root.join(FIXTURES).join("vault"), &dir);
    dir
}

/// Machine-specific values become placeholders.
fn normalise(mut v: Value, vault: &Path) -> Value {
    if let Some(o) = v.as_object_mut() {
        if o.contains_key("version") {
            o.insert("version".into(), "<version>".into());
        }
        if o.get("cwd").and_then(Value::as_str) == vault.to_str() {
            o.insert("cwd".into(), "<vault>".into());
        }
    }
    v
}

fn karamd(vault: &Path, args: &[&str]) -> Value {
    let out = Command::new(env!("CARGO_BIN_EXE_karamd"))
        .args(args)
        .arg("--vault")
        .arg(vault)
        .output()
        .unwrap();
    assert!(
        out.status.success(),
        "karamd {args:?}: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    normalise(serde_json::from_slice(&out.stdout).unwrap(), vault)
}

#[test]
fn macos_app_cli_contract() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR"));
    let vault = scratch_vault(root);
    let bless = std::env::var_os("KARAMD_BLESS").is_some();
    let cases: [(&str, &[&str]); 5] = [
        ("list", &["list", "open:true", "--json"]),
        ("show", &["show", "001", "--json"]),
        ("config", &["config", "--json"]),
        ("terminal-argv", &["terminal-argv", "001", "--json"]),
        (
            "create",
            &[
                "create",
                "Buy milk",
                "--phase",
                "now",
                "--today",
                "2026-01-15",
                "--json",
            ],
        ),
    ];
    for (name, args) in cases {
        let got = serde_json::to_string_pretty(&karamd(&vault, args)).unwrap() + "\n";
        let path = root.join(FIXTURES).join(format!("cli/{name}.json"));
        if bless {
            fs::create_dir_all(path.parent().unwrap()).unwrap();
            fs::write(&path, &got).unwrap();
        } else {
            let want = fs::read_to_string(&path).unwrap_or_default();
            assert_eq!(
                got, want,
                "{name} output changed; bless with KARAMD_BLESS=1 and update the macOS app"
            );
        }
    }
    fs::remove_dir_all(vault).unwrap();
}
