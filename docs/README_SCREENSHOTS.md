# README screenshot provenance

`assets/desktop-preview-v0.2.1.png` was captured from the actual Windows Tauri
Publishing Workbench v0.2.1 process on 2026-10-08, using the built-in sample
article. Source commit: `1a6d668c64` (full commit in the repository history).
The screenshot contains the native window and its rendered Markdown preview.
It demonstrates preview, not a remote publication or successful deployment.
No image compositing, UI replacement or AI generation was used.

To repeat on Windows:

1. Run `npm ci` and `npm run tauri -- build` from the repository root.
2. In a PowerShell session, set `$env:WORKBENCH_DATA_DIR` to a new empty test
   directory outside the repository. This isolates the screenshot session from
   personal articles, extension credentials and publishing history.
3. Run `src-tauri/target/release/publishing-workbench.exe` from that session.
4. Keep the built-in sample, open **Preview**, and wait for the Markdown heading
   and list to render. Do not configure a publishing target.
5. Capture just the native application window. Review all visible text for private
   paths or credentials before adding the image to documentation.

Preserve app icons and package branding. The screenshot uses first-party UI and
sample prose; repository license boundaries remain in LICENSE and LICENSE-CONTENT.
Re-capture only when the demonstrated workflow or released UI materially changes.
