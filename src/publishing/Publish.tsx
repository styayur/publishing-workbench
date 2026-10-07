import { useCallback, useEffect, useState } from 'react';
import { api, desktop, message } from '../api';
import type {
  Action,
  Content,
  Manifest,
  PublicationState,
  PublishJob,
} from '../models';
import { actions, supports } from './capabilities';
const empty: PublicationState = {
  jobs: [],
  mappings: [],
  plans: [],
  orphans: [],
};
export function Publish({
  manifests,
  content,
  connected,
  onTest,
  onFinished,
  onError,
}: {
  manifests: Manifest[];
  content: Content;
  connected: Record<string, boolean>;
  onTest: (id: string) => Promise<void>;
  onFinished: () => Promise<void>;
  onError: (e: unknown) => void;
}) {
  const [state, setState] = useState(empty);
  const [selected, setSelected] = useState<string[]>([]);
  const [modes, setModes] = useState<Record<string, Action>>({});
  const [busy, setBusy] = useState(false);
  const [results, setResults] = useState<Record<string, string>>({});
  const [confirmed, setConfirmed] = useState(false);
  const refresh = useCallback(async () => {
    if (desktop)
      setState(
        await api<PublicationState>('publication_state', {
          articleId: content.article_id,
        }),
      );
  }, [content.article_id]);
  useEffect(() => {
    void refresh().catch(onError);
    const timer = setInterval(() => void refresh().catch(onError), 1200);
    return () => clearInterval(timer);
  }, [refresh, onError]);
  const mode = (m: Manifest) =>
    modes[m.id] ?? (supports(m, 'createDraft') ? 'createDraft' : 'publish');
  const publicAction = selected.some((id) => {
    const m = manifests.find((m) => m.id === id)!;
    return ['publish', 'delete'].includes(mode(m));
  });
  async function run() {
    setBusy(true);
    try {
      for (const id of selected) {
        setResults((r) => ({ ...r, [id]: '执行中…' }));
        try {
          const m = manifests.find((m) => m.id === id)!;
          const job = await api<PublishJob>('dispatch', {
            id,
            action: mode(m),
            content,
          });
          setResults((r) => ({
            ...r,
            [id]: `${job.operation} · ${job.status}${job.error ? ' · ' + job.error.message : ''}`,
          }));
        } catch (e) {
          setResults((r) => ({ ...r, [id]: message(e) }));
        }
        await refresh();
      }
      await onFinished();
    } finally {
      setBusy(false);
      setConfirmed(false);
    }
  }
  async function task(command: string, args: Record<string, unknown>) {
    setBusy(true);
    try {
      await api(command, args);
      await refresh();
    } catch (e) {
      onError(e);
    } finally {
      setBusy(false);
    }
  }
  return (
    <>
      <div className="section-heading">
        <div>
          <h2>Publish</h2>
          <p>固定文章 ID，自动创建或更新。失败任务在 History 中恢复原快照。</p>
        </div>
      </div>
      <div className="publish-summary">
        <strong>{content.title || '尚未填写标题'}</strong>
        <small>{content.article_id}</small>
      </div>
      <div className="targets">
        {manifests.map((m) => {
          const plan = state.plans.find((p) => p.extension_id === m.id);
          const job = state.jobs.find(
            (j) =>
              j.article_id === content.article_id &&
              j.target_id === plan?.target_id,
          );
          const step = job?.steps.find(
            (s) =>
              s.status === 'running' ||
              s.status === 'failed' ||
              s.status === 'uncertain',
          );
          return (
            <section className="target" key={m.id}>
              <div className="target-title">
                <input
                  type="checkbox"
                  aria-label={`选择 ${m.name}`}
                  disabled={busy}
                  checked={selected.includes(m.id)}
                  onChange={(e) =>
                    setSelected(
                      e.target.checked
                        ? [...selected, m.id]
                        : selected.filter((id) => id !== m.id),
                    )
                  }
                />
                <div>
                  <h3>{m.name}</h3>
                  <p>
                    {plan?.operation ?? 'create'}
                    {plan?.remote_id ? ' · ID ' + plan.remote_id : ''}
                  </p>
                </div>
                <span className={connected[m.id] ? 'badge success' : 'badge'}>
                  {connected[m.id] ? '已连接' : '未验证'}
                </span>
              </div>
              <div className="target-controls">
                <span>
                  草稿：{m.capabilities.includes('draft') ? '支持' : '不支持'}
                </span>
                <span>
                  直接发布：
                  {m.capabilities.includes('publish') ? '支持' : '不支持'}
                </span>
                <select
                  aria-label={`${m.name} 操作`}
                  disabled={busy}
                  value={mode(m)}
                  onChange={(e) =>
                    setModes({ ...modes, [m.id]: e.target.value as Action })
                  }
                >
                  {actions
                    .filter(
                      (a) =>
                        supports(m, a.id) &&
                        (plan?.remote_id ||
                          !['update', 'delete'].includes(a.id)),
                    )
                    .map((a) => (
                      <option key={a.id} value={a.id}>
                        {a.label}
                      </option>
                    ))}
                </select>
                <button disabled={busy} onClick={() => onTest(m.id)}>
                  测试连接
                </button>
              </div>
              {job && (
                <p className="hint">
                  {job.status} · 素材复用 {job.assets_reused} / 上传{' '}
                  {job.assets_uploaded} · {step?.name ?? 'complete'}
                </p>
              )}
              {results[m.id] && (
                <p role="status" className="result">
                  {results[m.id]}
                </p>
              )}
            </section>
          );
        })}
      </div>
      {publicAction && (
        <label className="notice">
          <input
            type="checkbox"
            checked={confirmed}
            onChange={(e) => setConfirmed(e.target.checked)}
          />{' '}
          我已检查预览与目标，确认执行发布 / 删除；Git push
          可能触发仓库现有部署流程。
        </label>
      )}
      <div className="publish-bar">
        <span>已选择 {selected.length} 个目标</span>
        <button
          className="primary"
          disabled={
            !desktop ||
            busy ||
            !selected.length ||
            !content.title.trim() ||
            !content.body.trim() ||
            selected.some((id) => !connected[id]) ||
            (publicAction && !confirmed)
          }
          onClick={() => void run().catch(onError)}
        >
          {busy ? '执行中…' : '执行所选操作'}
        </button>
      </div>
      <h3>Publish History</h3>
      <p className="hint">
        重试使用任务中的内容和素材快照，不使用当前编辑内容。远端回执不确定时先核对
        ID；系统不会盲目创建重复文章。
      </p>
      <div className="history">
        {!state.jobs.length ? (
          <p>还没有发布记录。</p>
        ) : (
          state.jobs.map((j) => (
            <details key={j.job_id}>
              <summary>
                <strong>{j.content.title}</strong> ·{' '}
                {manifests.find((m) => m.id === j.extension_id)?.name ??
                  j.extension_id}{' '}
                · {j.operation} · {j.status} ·{' '}
                <time>{new Date(j.started_at * 1000).toLocaleString()}</time>
              </summary>
              <p>
                ID {j.receipt?.id ?? '等待回执'} · 尝试 {j.attempts} · 素材复用{' '}
                {j.assets_reused} / 上传 {j.assets_uploaded}
              </p>
              <ol>
                {j.steps.map((s) => (
                  <li key={s.name}>
                    {s.name} · {s.status} {s.detail}
                  </li>
                ))}
              </ol>
              {j.error && <p className="failure">{j.error.message}</p>}
              {!['success', 'running', 'cancelled'].includes(j.status) && (
                <div className="actions">
                  <button
                    disabled={busy}
                    onClick={() => task('retry_job', { jobId: j.job_id })}
                  >
                    Retry / Resume
                  </button>
                  {!['running', 'uncertain', 'success'].includes(
                    j.steps[3]?.status,
                  ) && (
                    <button
                      disabled={busy}
                      onClick={() => task('cancel_job', { jobId: j.job_id })}
                    >
                      取消此任务
                    </button>
                  )}
                </div>
              )}
              {['needs_reconciliation', 'interrupted'].includes(j.status) &&
                ['running', 'uncertain'].includes(j.steps[3]?.status) && (
                  <form
                    onSubmit={(e) => {
                      e.preventDefault();
                      const f = new FormData(e.currentTarget);
                      void task('reconcile_job', {
                        jobId: j.job_id,
                        remoteId: f.get('id'),
                        remoteStatus: f.get('status'),
                      });
                    }}
                  >
                    <label>
                      已在远端核对的 ID
                      <input name="id" required />
                    </label>
                    <label>
                      远端状态
                      <input name="status" defaultValue="draft" required />
                    </label>
                    <button disabled={busy}>保存核对结果，再 Resume</button>
                  </form>
                )}
            </details>
          ))
        )}
      </div>
      <h3>未关联 / 孤立素材</h3>
      <p className="hint">
        失败后保留素材映射。此处仅报告，不自动删除；pending / uncertain
        可能已上传，需要核对。
      </p>
      {state.orphans.map((a) => (
        <details key={a.asset_hash + a.target_id + a.variant}>
          <summary>
            {a.status} · {a.variant} · {a.asset_hash.slice(0, 12)} ·{' '}
            {a.target_id}
          </summary>
          <p>
            {a.remote_asset_id} {a.remote_url}
          </p>
          {['pending', 'uncertain'].includes(a.status) && (
            <form
              onSubmit={(e) => {
                e.preventDefault();
                const f = new FormData(e.currentTarget);
                void task('reconcile_asset', {
                  assetHash: a.asset_hash,
                  targetId: a.target_id,
                  variant: a.variant,
                  remoteAssetId: f.get('id'),
                  remoteUrl: f.get('url'),
                });
              }}
            >
              <label>
                已核对素材 ID
                <input name="id" />
              </label>
              <label>
                已核对素材 URL
                <input name="url" />
              </label>
              <button disabled={busy}>保存素材核对结果</button>
            </form>
          )}
        </details>
      ))}
    </>
  );
}
