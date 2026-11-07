# Security Policy

## Supported versions

Security fixes target the current `main` branch and the latest published prerelease line. v0.2.x is an alpha/preview line, not a stable-support promise.

## Report privately

Use GitHub private vulnerability reporting:

https://github.com/styayur/publishing-workbench/security/advisories/new

Do not include API tokens, application passwords, CMS content, private repository data, local databases, unpublished drafts, or full machine paths in a public issue.

## Security-sensitive areas

Relevant reports include credential handling, local SQLite/journal recovery, path traversal, import/export file access, Tauri IPC boundaries, remote CMS/Git adapter authorization, accidental force-push, asset upload, URL validation, and recovery logic that could duplicate or overwrite remote content.

## Operating boundary

The application is local-first. Remote writes occur only against explicitly configured adapters and require user action. Browser preview does not prove a remote publish succeeded. A timeout or missing receipt can mean the remote write occurred; use reconciliation rather than assuming rollback.

No automatic rollback, exactly-once guarantee across arbitrary remote APIs, stable support SLA, or security email is claimed.
