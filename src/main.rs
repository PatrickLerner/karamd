//! Binary shim. All logic lives in the `karamd` library crate so it stays
//! unit-testable; `main` only wires in the process arguments. The `ExitCode`
//! pass-through lets `validate` exit 2 on warnings under `--strict`.

fn main() -> anyhow::Result<std::process::ExitCode> {
    karamd::run(std::env::args_os())
}
