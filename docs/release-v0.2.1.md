# v0.2.1 — Reliable Publishing MVP image fix

Recommended pre-release for the v0.2 Publishing Workbench. It includes all v0.2.0 features and fixes Git Content asset URL replacement for reference-style Markdown images and raw HTML img tags. Image alt text is retained; code examples are left intact. Existing v0.2.0 tags are not rewritten. Content hashes include the transform version, so republishing after this upgrade updates the mapped file instead of skipping the correction.

Validation: 32 Rust tests, 7 frontend tests, formatter/clippy/lint/build and real Windows WebView2 / GUI smoke passed. The original v0.2 integration also passed the existing Digital Garden publication gate: a local draft was excluded from staging and public output, with no production commit/push/deployment.

Download the Windows x64 portable ZIP and run publishing-workbench.exe. Requires WebView2 Runtime. EXE is unsigned; macOS/Linux not validated. Source, SHA-256 checksums and third-party license inventory/texts accompany the release.

Application code: AGPL-3.0-only; first-party prose CC BY 4.0; third-party terms unchanged. Known risks: uncertain CMS responses may need manual reconciliation, REST idempotence depends on the server, WP lacks a conditional revision lock, Git should use a dedicated checkout, and real WP/WeChat accounts were not exercised. See README and docs/reliability.md.
