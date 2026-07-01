//! Binary shim. All logic lives in the `karamd` library crate so it stays
//! unit-testable; `main` only wires in the process arguments.

fn main() -> anyhow::Result<()> {
    karamd::run(std::env::args_os())
}
