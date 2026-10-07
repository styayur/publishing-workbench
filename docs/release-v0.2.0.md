# v0.2.0 — Reliable Publishing MVP

This pre-release extends the existing Tauri/Rust/React workbench with durable local publishing state and a fourth destination, Git Content.

- Stable article UUIDs, SQLite migration/versioning, remote mapping, checkpoint journal and Retry/Resume.
- SHA-256 asset registry and per-target deduplication; uncertain remote results stop for reconciliation instead of blindly creating duplicates.
- Git Content with optional commit/push, before/after recovery snapshots, dirty-tree checks, MD/MDX and Digital Garden Engine profile.
- Publish History, capability-driven operations, schema-generated Git settings, repository suggestions and local-image preview.
- WordPress, WeChat draft-only, and Generic REST integrated with the reliable core.

Validation: 31 Rust tests, 7 frontend tests, formatter/clippy/lint/build, real Windows WebView2 smoke, GUI smoke and the existing site's publication gate passed. Site verification created a local draft only; no production content, Git push or deployment occurred.

Download the Windows x64 portable ZIP and run publishing-workbench.exe. WebView2 Runtime is required. EXE is unsigned. Verify SHA256SUMS.txt. Source is included; macOS/Linux are not validated in this release.

Application code: AGPL-3.0-only. First-party prose: CC BY 4.0. Third-party licenses retained and collected alongside the binary.

Known limitations: unknown CMS responses may require manual reconciliation; Generic REST idempotence depends on its server; WP has no conditional revision lock; use a dedicated Git checkout; caches require backup; real WP/WeChat accounts were not exercised. See README and docs/reliability.md.
