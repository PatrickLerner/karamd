# Changelog

All notable changes to karamd are documented here. The format follows
[Keep a Changelog](https://keepachangelog.com/en/1.1.0/) and the project adheres
to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [0.1.1] - 2026-07-01

### Added

- `--config` now defaults to `<vault>/.taskmd.recurring.yaml` when omitted, so a
  rules file kept next to `.taskmd.yaml` needs no flag and unattended runs pass
  only `--vault`.

## [0.1.0] - 2026-07-01

### Added

- Initial release: `generate` command with `after_completion` and `calendar`
  triggers, idempotent creation via a `recurring:` frontmatter marker, `--dry-run`
  and `--today` overrides, Nix flake, and CI.

[0.1.1]: https://github.com/PatrickLerner/karamd/compare/v0.1.0...v0.1.1
[0.1.0]: https://github.com/PatrickLerner/karamd/releases/tag/v0.1.0
