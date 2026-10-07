import { describe, it, expect } from 'vitest';
import { importContent } from './editor/import';
import { supports } from './publishing/capabilities';
import type { Manifest } from './models';
describe('content import', () => {
  it('imports Markdown and extracts heading', () => {
    expect(importContent('# Hello\n\nText', false).title).toBe('Hello');
  });
  it('preserves canonical JSON fields', () => {
    const c = importContent(
      JSON.stringify({
        title: 'Hello',
        body: 'Text',
        tags: ['a'],
        metadata: { categories: [1] },
      }),
      true,
    );
    expect(c.metadata).toEqual({ categories: [1] });
    expect(c.tags).toEqual(['a']);
  });
  it('rejects invalid field types', () => {
    for (const value of [
      { body: 12 },
      { tags: 'a' },
      { metadata: [] },
      { authors: [1] },
      { cover: 123 },
    ])
      expect(() => importContent(JSON.stringify(value), true)).toThrow();
  });
});
describe('capability-driven UI', () => {
  const m = { capabilities: ['draft', 'html'] } as Manifest;
  it('exposes drafts and hides unsupported actions', () => {
    expect(supports(m, 'createDraft')).toBe(true);
    expect(supports(m, 'publish')).toBe(false);
    expect(supports(m, 'update')).toBe(false);
  });
});

describe('v0.2 identity and garden frontmatter', () => {
  it('preserves article identity when roundtripping exported JSON', () => {
    const a = importContent('# Title', false);
    const b = importContent(JSON.stringify({ ...a, title: 'Changed' }), true);
    expect(b.article_id).toBe(a.article_id);
    expect(importContent('# Title', false).article_id).not.toBe(a.article_id);
  });
  it('imports YAML frontmatter without exposing it in the body', () => {
    const a = importContent(
      '---\ntitle: Garden\nslug: garden\ndescription: A note\ntags: [a, b]\nmaturity: budding\n---\n# Body',
      false,
    );
    expect(a.title).toBe('Garden');
    expect(a.slug).toBe('garden');
    expect(a.summary).toBe('A note');
    expect(a.tags).toEqual(['a', 'b']);
    expect(a.metadata.maturity).toBe('budding');
    expect(a.body).toBe('# Body');
  });
  it('supports Git operations from capabilities only', () => {
    const m = {
      capabilities: ['draft', 'publish', 'update', 'delete', 'mdx', 'revision'],
    } as Manifest;
    expect(supports(m, 'delete')).toBe(true);
    expect(supports(m, 'update')).toBe(true);
  });
});
