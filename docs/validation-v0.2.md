# v0.2.0 verification report

验证日期：2026-10-08，Windows x64。本报告记录本地执行结果，不代表真实 WP/微信账号或生产部署已经验证。

## Application checks

- `cargo fmt --check` / `cargo clippy --all-targets -- -D warnings`：通过。
- `cargo test`：31 tests，16 原适配器回归 + 15 Reliable Publishing tests，全部通过。
- `npm test`：7 tests 通过。
- `npm run lint`：ESLint / Prettier 通过。
- `npm run build`：TypeScript / Vite 通过。
- `npm run tauri -- build`：Windows release EXE 构建成功。
- Python Playwright GUI smoke：Editor、schema 表单、secret 清空显示、capability-driven actions、多目标部分失败、History、sandbox Preview、Settings、小屏布局，无 JavaScript 错误。
- Native WebView2 smoke：实际 Release EXE + Tauri IPC + Rust loopback HTTP；本地保存、local-image data-URL preview、路径导入复用 ID、自动 DGE 探测、REST HEAD、create→update、SQLite mapping/journal 和无密钥磁盘字段全部通过。使用 `WORKBENCH_DATA_DIR` 隔离临时 profile，未改用户正常工作区。

## Reliable tests

稳定 UUID / JSON roundtrip、SHA-256、schema migration/version refusal/file lock/source identity、REST create→update→unchanged、认证修正后同 job retry、未知 create 不再发送请求、人工 remote receipt 核对、receipt 后断电恢复、Git dirty tree 拒绝、文件创建/更新/MDX/promotion/delete、frontmatter 三种 kind、draft/public 路径边界、WP body/cover 素材去重与恢复、微信素材/草稿 create→update、Git asset scope/dedup、push 失败后同事务恢复且 HEAD 不产生重复 commit、REST Idempotency-Key 契约。

## styayur.co.uk safe integration

通过本地 Git remote、site.config.ts 和 engine.lock.json 确认了对应内容仓库。仅创建并更新测试文件 `content/drafts/posts/workbench-v02-safe-draft.mdx`，status=draft，title/slug/date/description/tags/maturity 均有效。使用固定 article_id 映射从 create 转为 update。

- Git status 只有该新增 untracked draft；使用 `git diff --no-index -- NUL <draft>` 检查生成文件。差异命令预期 exit=1 表示有 diff。
- 未 commit、push；未更新真实公开文章，也未调用 Wrangler。
- 锁定引擎 `styayur/digital-garden-engine@v0.2.0`，本地 checkout commit `7027bd580bed53bb194167abec06c3f6e01c7206`。
- validation：33 published / 1 draft / 0 private，通过。
- staging：33 published entries / 14 public assets；1 draft 被排除。
- public build：Next.js 静态构建成功，594 内部链接通过。
- artifact audit：273 文件通过，无 forbidden canary/secrets/admin/private fingerprint/source maps。
- 在 staging 与 public 输出搜索 `WORKBENCH_V02_DRAFT_BOUNDARY_CANARY` 和测试 slug，均没有匹配。
- 引擎 publication-boundary 自测：2 tests 通过，包括 draft/private canary 排除与未知 status fail-closed。

测试草稿保留在本地内容仓库供审阅；其状态数据库与公开构建输出保存在工作台 gitignored test-results 中，不上传 GitHub。远端 GitHub Actions / Cloudflare Pages / 线上站点没有被本次测试触发。

## Release assessment

适合发布 **v0.2.0 MVP 预览版**，不能承诺所有外部 CMS 的 exactly-once、跨平台安装包或生产账号验证。GitHub Release 标为 pre-release，Windows portable 未签名。主要生产风险及下一版 5 项优先工作见 reliability.md。

## Reproduce GUI/native checks

安装 Python Playwright 后：

```sh
python -m pip install playwright
python -m playwright install chromium
cargo run --manifest-path src-tauri/Cargo.toml --example manifests > tests/manifests.fixture.json
npm run dev
# separate terminal
python tests/gui_smoke.py
# release EXE must exist; Windows WebView2; uses a temporary profile
python tests/native_smoke.py
```

Git fixture测试仅使用临时仓库与本地 bare remote；API mock 测试没有真实认证信息或外部账号请求。
