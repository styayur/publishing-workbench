import { useCallback, useEffect, useState } from 'react';
import { marked } from 'marked';
import { api, desktop, message } from './api';
import {
  sample,
  type Content,
  type Manifest,
  type Prepared,
  type Workspace,
} from './models';
import { Editor } from './editor/Editor';
import { Preview } from './preview/Preview';
import { Publish } from './publishing/Publish';
import { Extensions } from './extensions/Extensions';
import { Settings } from './settings/Settings';
const pages = [
  'Editor',
  'Preview',
  'Publish',
  'Extensions',
  'Settings',
] as const;
type Page = (typeof pages)[number];
export default function App() {
  const [page, setPage] = useState<Page>('Editor');
  const [content, setContent] = useState<Content>(sample);
  const [manifests, setManifests] = useState<Manifest[]>([]);
  const [workspace, setWorkspace] = useState<Workspace>({
    content: sample,
    extensions: {},
    asset_directory: '',
    history: [],
  });
  const [directory, setDirectory] = useState('');
  const [connected, setConnected] = useState<Record<string, boolean>>({});
  const [error, setError] = useState('');
  const [status, setStatus] = useState('尚未保存');
  const [preview, setPreview] = useState<Prepared | null>(null);
  const [target, setTarget] = useState('');
  const [testing, setTesting] = useState('');
  const [ready, setReady] = useState(!desktop);
  const onError = useCallback((e: unknown) => setError(message(e)), []);
  const reload = useCallback(async () => {
    const ws = await api<Workspace>('load_workspace');
    setWorkspace(ws);
  }, []);
  useEffect(() => {
    if (!desktop) return;
    let active = true;
    Promise.all([
      api<Manifest[]>('manifests'),
      api<Workspace>('load_workspace'),
    ])
      .then(([m, ws]) => {
        if (!active) return;
        setManifests(m);
        setWorkspace(ws);
        setContent(
          ws.content.body || ws.content.title
            ? ws.content
            : { ...sample, article_id: ws.content.article_id },
        );
        setDirectory(ws.asset_directory);
        setReady(true);
        setStatus('已加载本地工作区');
      })
      .catch(onError);
    return () => {
      active = false;
    };
  }, [onError]);
  useEffect(() => {
    if (page !== 'Preview') return;
    let active = true;
    const timer = setTimeout(() => {
      const promise = desktop
        ? api<Prepared>('preview', { content, id: target || null })
        : Promise.resolve({
            content,
            html: marked.parse(content.body, { async: false }),
            warnings: [],
          });
      promise
        .then((p) => {
          if (active) setPreview(p);
        })
        .catch(onError);
    }, 200);
    return () => {
      active = false;
      clearTimeout(timer);
    };
  }, [page, content, target, onError]);
  async function test(id: string) {
    setTesting(id);
    setError('');
    try {
      await api('test_connection', { id });
      setConnected((c) => ({ ...c, [id]: true }));
      setStatus('连接测试成功');
    } catch (e) {
      setConnected((c) => ({ ...c, [id]: false }));
      onError(e);
    } finally {
      setTesting('');
    }
  }
  async function save() {
    try {
      await api('save_content', {
        content: {
          ...content,
          tags: content.tags.filter(Boolean),
          authors: content.authors.filter(Boolean),
        },
        assetDirectory: directory,
      });
      setStatus('已保存到本机');
      setError('');
    } catch (e) {
      onError(e);
    }
  }
  function exportContent() {
    const url = URL.createObjectURL(
      new Blob([JSON.stringify(content, null, 2)], {
        type: 'application/json',
      }),
    );
    const a = document.createElement('a');
    a.href = url;
    a.download = `${content.slug.replace(/[^\w-]/g, '') || 'article'}.json`;
    a.click();
    setTimeout(() => URL.revokeObjectURL(url), 1000);
  }
  return (
    <div className="shell">
      <aside>
        <div className="brand">
          <span className="brand-mark">P</span>
          <div>
            Publishing
            <br />
            <strong>Workbench</strong>
          </div>
        </div>
        <nav>
          {pages.map((p, i) => (
            <button
              key={p}
              className={page === p ? 'active' : ''}
              onClick={() => setPage(p)}
            >
              <span className="nav-icon" aria-hidden="true">
                {['✎', '◫', '↗', '⊞', '⚙'][i]}
              </span>
              {p}
            </button>
          ))}
        </nav>
        <div className="sidebar-foot">
          <span className="local-dot" /> 本地优先
          <p>
            Markdown in.
            <br />
            Your destinations out.
          </p>
          <small>v0.2.1 · Reliable publishing</small>
        </div>
      </aside>
      <div className="main">
        <header>
          <div>
            <span className="document-dot" /> 当前文章{' '}
            <strong>{content.slug || 'untitled'}</strong>
          </div>
          <div className="header-actions">
            <small role="status">{status}</small>
            <button disabled={!desktop || !ready} onClick={save}>
              保存本地
            </button>
          </div>
        </header>
        <main>
          {!desktop && (
            <p className="notice">
              浏览器编辑演示。真实扩展配置与发布请通过 npm run tauri dev
              启动桌面应用。
            </p>
          )}
          {error && (
            <div role="alert" className="error">
              {error}
              <button aria-label="关闭错误" onClick={() => setError('')}>
                ×
              </button>
            </div>
          )}
          {testing && (
            <p role="status" className="notice">
              正在测试 {manifests.find((m) => m.id === testing)?.name}…
            </p>
          )}
          {!ready ? (
            <p>正在读取本地工作区…</p>
          ) : (
            <>
              {page === 'Editor' && (
                <Editor
                  content={content}
                  onChange={(c) => {
                    setContent(c);
                    setStatus('有未保存的修改');
                  }}
                  onError={onError}
                />
              )}
              {page === 'Preview' && (
                <>
                  <div className="section-heading">
                    <div>
                      <h2>Preview</h2>
                      <p>预览只转换内容；图片会在实际提交时上传。</p>
                    </div>
                    <select
                      aria-label="预览扩展"
                      value={target}
                      onChange={(e) => setTarget(e.target.value)}
                    >
                      <option value="">标准 HTML</option>
                      {manifests.map((m) => (
                        <option value={m.id} key={m.id}>
                          {m.name}
                        </option>
                      ))}
                    </select>
                  </div>
                  {preview ? (
                    <Preview html={preview.html} warnings={preview.warnings} />
                  ) : (
                    <p>正在生成预览…</p>
                  )}
                </>
              )}
              {page === 'Publish' && (
                <Publish
                  manifests={manifests}
                  content={{
                    ...content,
                    tags: content.tags.filter(Boolean),
                    authors: content.authors.filter(Boolean),
                  }}
                  connected={connected}
                  onTest={test}
                  onFinished={reload}
                  onError={onError}
                />
              )}
              {page === 'Extensions' && (
                <Extensions
                  manifests={manifests}
                  configs={workspace.extensions}
                  connected={connected}
                  onTest={test}
                  onError={onError}
                  onSaved={async () => {
                    await reload();
                    setConnected({});
                    setStatus('配置已保存，请重新测试连接');
                  }}
                />
              )}
              {page === 'Settings' && (
                <Settings
                  directory={directory}
                  onChange={(d) => {
                    setDirectory(d);
                    setStatus('有未保存的修改');
                  }}
                  onExport={exportContent}
                />
              )}
            </>
          )}
        </main>
        <footer>
          <span>Canonical content · Markdown</span>
          <span>{content.body.length.toLocaleString()} 字符</span>
        </footer>
      </div>
    </div>
  );
}
