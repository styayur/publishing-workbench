# Reliable Publishing v0.2

## SQLite schema (migration 001)

直接 SQL + rusqlite，无 ORM。完整 SQL：`src-tauri/storage/001.sql`。

| Table           | Key                              | State                                                                   |
| --------------- | -------------------------------- | ----------------------------------------------------------------------- |
| articles        | article_id UUID                  | content_json snapshot, source_path, updated_at                          |
| targets         | target_id                        | extension_id, non-secret config_json                                    |
| remote_mappings | article_id + target_id           | remote_id, remote_revision, last_published_hash, status                 |
| assets          | asset_hash + target_id + variant | remote_asset_id, remote_url, status, created_by_job, linked             |
| publish_jobs    | job_id UUID                      | article_id, target_id, extension_id, operation, status, times, job_json |
| publish_steps   | job_id + sequence                | name, status, detail                                                    |
| settings        | key                              | workspace settings for v0.1 compatibility                               |

WAL + synchronous FULL + busy timeout 5s；进程文件锁阻止多个应用实例使用同一状态目录。user_version 0→1 在事务中建立表/索引；较新版本拒绝打开。重启将 running 标记 interrupted。部分唯一索引阻止同一 article/target 同时存在多个未结束任务。正文源文件独立；JSON/MDX 导出和路径导入保留文件工作流。

## 状态机与恢复

Job: running / failed / needs_reconciliation / interrupted / success / cancelled。Step: pending / running / failed / uncertain / success。Operation: create / update / delete / unchanged；retry 为相同 job 的新 attempt，不伪装成新的 create。

六步分别 checkpoint，成功步骤不重复。变换时保存内容、素材字节快照；远端返回 receipt 先持久化，mapping 单独写入。save mapping 失败重试使用已保存的 receipt，不重发 create。mapping 和 job 不是同一跨网络事务，但先 receipt 后 mapping 的顺序消除已知回执的重复请求。

远端请求期间断电或连接断开意味着结果不确定。必须 recover、服务器 Idempotency-Key，或人工绑定已核实的回执。没有凭空承诺跨所有 CMS exactly-once。未核对远端的任务不能取消后绕过为新 create；asset pending/uncertain 同样阻止再次上传。已知认证错误可修正密钥重试。改变 endpoint/template/paths 等非密钥配置不允许恢复原任务；可安全取消尚未成功/未知远端请求的任务后建立新任务。配置中的 secret 字段不参与重试哈希。

WP 在 content.raw 中查找 article/job marker；微信无可靠远端查询时手动核对；REST 默认不声明安全重发；Git 的 before/after 文件事务可以恢复已写文件与失败 commit/push。Git 外部用户修改会中止事务而不是覆盖。

## 素材与孤立素材

哈希基于内容字节；不同本地路径但同一字节、同一目标/variant 复用一个映射。微信封面/正文 API 语义不同，因此 variant 分开；Git 以 status 隔离。available 映射立即落盘，失败后继续复用；pending/uncertain 回执须人工查远端。任务成功后标记 linked。未关联映射表示可能孤立，不等于可以删除。v0.2 不提供自动远端清理；Git staging 位于 .git 元数据目录，未执行正文事务的缓存不构成公开素材。

## 当前生产风险

- WordPress / 微信真实账号没有在本次测试中发送请求；权限、白名单、代理、平台限额仍需目标环境验证。
- 外部服务缺少幂等协议时未知响应需要人工核对；不可用的远端素材没有自动失效检测。
- SQLite / asset cache / Git transaction snapshots 需要共同备份。缓存尚无保留期限、GC 或应用层加密。
- Git 文件校验与写入不是与其他编辑器共享的原子锁；应使用专用 checkout，避免并发编辑、push。自动 push 会触发现有仓库 CI，应谨慎配置。
- WP revision 被记录但没有条件更新锁。Generic REST 的幂等性是服务端契约，用户误声明会破坏保证。
- DGE profile 严格针对现有 schema；引擎升级前应跑 validation。本地校验不等于已经通过远端 GitHub Actions 或线上部署。
- Windows portable 发行未签名，macOS/Linux 未验证，无自动升级或数据库降级支持。

## 下一版优先项

1. 增加 provider-specific 查询/对账页面，降低微信和未知 REST 手工核对成本。
2. WP 条件更新、Git repository lock 和可审核的冲突解决。
3. 映射导入/备份恢复、缓存保留期限与可确认的孤立素材清理。
4. Git profile 调用内容仓库自己的验证命令，发布前显示 frontmatter/diff。
5. 签名安装包、跨平台 CI 和受控真实账号端到端测试。
