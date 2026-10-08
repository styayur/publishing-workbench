import { sample, type Content } from '../models';
import { parse } from 'yaml';
const uuid = /^[0-9a-f]{8}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{12}$/i;
export function importContent(text: string, json: boolean): Content {
  if (!json) {
    let metadata: Record<string, unknown> = {};
    let body = text;
    const match = text.match(/^---\r?\n([\s\S]*?)\r?\n---(?:\r?\n|$)/);
    if (match) {
      const raw: unknown = parse(match[1]);
      if (!raw || typeof raw !== 'object' || Array.isArray(raw))
        throw Error('Frontmatter 必须为对象');
      metadata = raw as Record<string, unknown>;
      if (
        metadata.article_id !== undefined &&
        (typeof metadata.article_id !== 'string' ||
          !uuid.test(metadata.article_id))
      )
        throw Error('article_id 必须为 UUID');
      body = text.slice(match[0].length);
    }
    const articleMarker = body.match(
      /<!-- workbench article:([0-9a-f-]{36}) -->/i,
    )?.[1];
    body = body
      .replace(/^\s*<!-- workbench article:[0-9a-f-]{36} -->\s*$/gm, '')
      .trimEnd();
    return {
      ...sample,
      article_id:
        typeof metadata.article_id === 'string'
          ? metadata.article_id
          : articleMarker && uuid.test(articleMarker)
            ? articleMarker
            : crypto.randomUUID(),
      title:
        typeof metadata.title === 'string'
          ? metadata.title
          : (body.match(/^#\s+(.+)$/m)?.[1] ?? '导入的文章'),
      body,
      slug: typeof metadata.slug === 'string' ? metadata.slug : '',
      summary:
        typeof metadata.description === 'string' ? metadata.description : '',
      cover: typeof metadata.image === 'string' ? metadata.image : null,
      authors: [],
      tags:
        Array.isArray(metadata.tags) &&
        metadata.tags.every((v) => typeof v === 'string')
          ? metadata.tags
          : [],
      metadata,
    };
  }
  const value: unknown = JSON.parse(text);
  if (typeof value !== 'object' || !value || Array.isArray(value))
    throw Error('文章 JSON 必须为对象');
  const v = value as Record<string, unknown>;
  for (const field of ['article_id', 'title', 'slug', 'summary', 'body'])
    if (v[field] !== undefined && typeof v[field] !== 'string')
      throw Error(`${field} 必须为字符串`);
  for (const field of ['authors', 'tags'])
    if (
      v[field] !== undefined &&
      (!Array.isArray(v[field]) ||
        !(v[field] as unknown[]).every((s) => typeof s === 'string'))
    )
      throw Error(`${field} 必须为字符串数组`);
  if (v.cover !== undefined && v.cover !== null && typeof v.cover !== 'string')
    throw Error('cover 必须为字符串或 null');
  if (
    v.metadata !== undefined &&
    (typeof v.metadata !== 'object' ||
      v.metadata === null ||
      Array.isArray(v.metadata))
  )
    throw Error('metadata 必须为对象');
  if (
    v.article_id !== undefined &&
    (typeof v.article_id !== 'string' || !uuid.test(v.article_id))
  )
    throw Error('article_id 必须为 UUID');
  if (
    v.format !== undefined &&
    !['markdown', 'mdx'].includes(v.format as string)
  )
    throw Error('format 必须为 markdown 或 mdx');
  if (
    v.source_path !== undefined &&
    v.source_path !== null &&
    typeof v.source_path !== 'string'
  )
    throw Error('source_path 必须为字符串或 null');
  return {
    ...sample,
    article_id: crypto.randomUUID(),
    title: '',
    slug: '',
    summary: '',
    body: '',
    cover: null,
    authors: [],
    tags: [],
    metadata: {},
    ...v,
  } as Content;
}
