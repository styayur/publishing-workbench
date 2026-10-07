import { useEffect, useRef, useState } from 'react';
import type { Content } from '../models';
import { api, desktop } from '../api';
import { importContent } from './import';
export function Editor({
  content,
  onChange,
  onError,
}: {
  content: Content;
  onChange: (c: Content) => void;
  onError: (e: unknown) => void;
}) {
  const [source, setSource] = useState('');
  useEffect(
    () => setMetadata(JSON.stringify(content.metadata, null, 2)),
    [content.article_id, content.metadata],
  );
  const input = useRef<HTMLInputElement>(null);
  const [metadata, setMetadata] = useState(
    JSON.stringify(content.metadata, null, 2),
  );
  const set = (key: keyof Content, value: unknown) =>
    onChange({ ...content, [key]: value });
  return (
    <>
      <div className="section-heading">
        <div>
          <h2>Editor</h2>
          <p>专注写作，发布目标稍后选择。</p>
        </div>
        <button onClick={() => input.current?.click()}>
          导入 MD / MDX / JSON
        </button>
        <input
          ref={input}
          type="file"
          accept=".md,.mdx,.markdown,.json,.txt"
          hidden
          onChange={async (e) => {
            const file = e.target.files?.[0];
            if (!file) return;
            try {
              const c = importContent(
                await file.text(),
                file.name.endsWith('.json'),
              );
              if (file.name.endsWith('.mdx')) c.format = 'mdx';
              onChange(c);
              setMetadata(JSON.stringify(c.metadata, null, 2));
            } catch (e) {
              onError(e);
            }
            e.target.value = '';
          }}
        />
      </div>
      <p className="hint">
        Article ID: {content.article_id} · 编辑标题不会改变 ID。
        <button
          onClick={() =>
            onChange({
              ...content,
              article_id: crypto.randomUUID(),
              source_path: null,
              title: '',
              slug: '',
              body: '',
              metadata: {},
            })
          }
        >
          新建文章
        </button>
      </p>
      {desktop && (
        <details>
          <summary>从本地路径导入（重复导入保留文章 ID）</summary>
          <label>
            MD / MDX / JSON 文件路径
            <input value={source} onChange={(e) => setSource(e.target.value)} />
          </label>
          <button
            onClick={async () => {
              try {
                const raw = await api<string>('read_content_file', {
                  path: source,
                });
                const c = importContent(raw, source.endsWith('.json'));
                if (source.endsWith('.mdx')) c.format = 'mdx';
                onChange(
                  await api<Content>('import_file', {
                    path: source,
                    content: c,
                  }),
                );
              } catch (e) {
                onError(e);
              }
            }}
          >
            导入本地文件
          </button>
        </details>
      )}
      <label>
        正文格式
        <select
          value={content.format || 'markdown'}
          onChange={(e) => set('format', e.target.value)}
        >
          <option value="markdown">Markdown</option>
          <option value="mdx">MDX（Git；预览不执行 JSX）</option>
        </select>
      </label>
      <label>
        文章标题
        <input
          className="title-input"
          value={content.title}
          onChange={(e) => set('title', e.target.value)}
        />
      </label>
      <div className="fields">
        <label>
          Slug
          <input
            value={content.slug}
            onChange={(e) => set('slug', e.target.value)}
          />
        </label>
        <label>
          作者（逗号分隔）
          <input
            value={content.authors.join(', ')}
            onChange={(e) =>
              set(
                'authors',
                e.target.value.split(',').map((s) => s.trim()),
              )
            }
          />
        </label>
      </div>
      <label>
        摘要
        <textarea
          rows={2}
          value={content.summary}
          onChange={(e) => set('summary', e.target.value)}
        />
      </label>
      <label className="body-label">
        <span>
          正文 <small>Markdown</small>
        </span>
        <textarea
          className="markdown-editor"
          spellCheck={false}
          value={content.body}
          onChange={(e) => set('body', e.target.value)}
        />
      </label>
      <div className="fields">
        <label>
          封面图片路径 / URL
          <input
            value={content.cover ?? ''}
            placeholder="https://… 或 C:/images/cover.jpg"
            onChange={(e) => set('cover', e.target.value || null)}
          />
        </label>
        <label>
          标签（逗号分隔）
          <input
            value={content.tags.join(', ')}
            onChange={(e) =>
              set(
                'tags',
                e.target.value.split(',').map((s) => s.trim()),
              )
            }
          />
        </label>
      </div>
      <details>
        <summary>Metadata（JSON）</summary>
        <textarea
          rows={5}
          value={metadata}
          onChange={(e) => setMetadata(e.target.value)}
          onBlur={() => {
            try {
              const v: unknown = JSON.parse(metadata);
              if (!v || typeof v !== 'object' || Array.isArray(v))
                throw Error('Metadata 必须为对象');
              set('metadata', v);
            } catch (e) {
              onError(e);
            }
          }}
        />
      </details>
    </>
  );
}
