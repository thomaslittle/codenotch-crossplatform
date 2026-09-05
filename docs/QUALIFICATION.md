# Qualification status

This repository was assembled in a constrained build sandbox on 2026-09-05.

## Checks completed locally

- TypeScript/TSX syntax transpilation: 11 files, 0 diagnostics.
- JSON/config parsing: all JSON files parsed successfully.
- Usage threshold/headline formatting smoke checks: 8 assertions passed.
- Basic secret-pattern scan across source/config/docs: clean.
- Static visual preview reviewed after the final inverse-flare and four-provider layout pass.

## Checks that require CI or a development machine

The assembly sandbox does not contain a Rust toolchain and does not have the npm dependency cache/network access required to install this project's dependencies. Because of that, the following are intentionally **not** claimed as having run locally:

```text
npm run typecheck
npm test
npm run build
cargo test --manifest-path src-tauri/Cargo.toml
npm run tauri build
```

`.github/workflows/ci.yml` runs that full qualification matrix on both `windows-latest` and `ubuntu-24.04`. Do not publish a release until both jobs are green.
