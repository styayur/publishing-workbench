# Architecture evidence

Source review: `2dfc1b516e1b0a544b067c70f87542a080ada7ab` (2026-10-09).

The marked Mermaid block in [README](../../README.md) is the only maintained diagram source. GitHub renders it natively in the reader's theme. No duplicate SVG or independent `.mmd` is committed; extracted Mermaid and SVG files are disposable verification artifacts.

桌面 `dispatch` 先进入 `ReliablePublishing::start`，后者选择 Publisher 并调用 transform；不是前端先完成平台转换。内置 Registry 是 Rust trait 分派，不是独立插件进程。SQLite 保存任务、步骤、映射和素材，凭据由桌面端从 OS keyring 读取。

失败后 Retry 使用原任务快照。未知远端结果进入 `needs_reconciliation`，不能保证任意外部 API 恰好执行一次；Git Content 使用文件事务核对恢复。启动把遗留 running 任务标为 interrupted。工作台不执行网站生产部署；可选 Git push 后的 CI 属于目标仓库。现有截图仅证明 v0.2.1 原生预览，后续 CMS 代码不因此被宣称已发布。

## Source map

- [src/api.ts](../../src/api.ts): `invoke<T>`
- [src-tauri/desktop.rs](../../src-tauri/desktop.rs): `async fn dispatch`, `async fn retry_job`, `keyring::Entry`
- [src-tauri/content/mod.rs](../../src-tauri/content/mod.rs): `pub struct Content`
- [src-tauri/publishing/reliable.rs](../../src-tauri/publishing/reliable.rs): `publisher.transform`, `publisher.recover`, `needs_reconciliation`
- [src-tauri/storage/mod.rs](../../src-tauri/storage/mod.rs): `interrupted`, `pub fn save_job`
- [src-tauri/extensions/mod.rs](../../src-tauri/extensions/mod.rs): `pub fn builtins`
- [src-tauri/extensions/git_content/mod.rs](../../src-tauri/extensions/git_content/mod.rs): `fn safe_remote_retry`, `transactions`
- [src-tauri/tests/reliable.rs](../../src-tauri/tests/reliable.rs): `#[tokio::test]`

The anchors in `evidence.json` catch renamed/deleted source symbols; they do not prove call semantics. The source review above checked the actual call sites and boundaries. A significant change to data flow, persistence, authentication, recovery or process boundaries requires reviewing this diagram and updating the evidence. Routine edits do not require redrawing it.

- [src-tauri/publishing/mod.rs](../../src-tauri/publishing/mod.rs): `pub struct Registry`, `pub trait Publisher`

## Verification

Requires Python 3, Node.js 22+ and network access for the documentation-only Mermaid CLI. From the repository root:

```sh
python docs/architecture/verify.py --render
```

This checks local README image references and source anchors, extracts the authoritative block, renders it twice with Mermaid CLI 11.12.0 using deterministic IDs, compares SVG bytes, validates SVG XML, and also renders the dark theme. If the bundled browser is unavailable, pass `--chrome /absolute/path/to/chrome` (or set `PUPPETEER_EXECUTABLE_PATH`). The CLI version is pinned; its transitive npm dependencies and the browser are environment-dependent, so the byte comparison proves repeatability within the same installed toolchain. Output goes to a temporary directory, never application runtime dependencies. GitHub Markdown/browser rendering still requires visual review; CLI validation alone is not evidence of GitHub rendering.

GitDiagram returned an initial diagram on 2026-10-09 for the public repository as a discovery aid. Its generated output is not imported as authoritative architecture or licensed artwork. No private source, config or credentials were submitted.

Existing repository licenses and third-party notices continue to apply. These diagrams are documentation authored from this repository's public source; no app icons, installer assets or third-party marks are replaced.
