import type { Action, Manifest } from '../models';
export const actions: { id: Action; capability: string; label: string }[] = [
  { id: 'createDraft', capability: 'draft', label: '创建草稿' },
  { id: 'publish', capability: 'publish', label: '直接发布' },
  { id: 'update', capability: 'update', label: '更新文章' },
  { id: 'delete', capability: 'delete', label: '删除映射文件' },
];
export const supports = (manifest: Manifest, action: Action) =>
  manifest.capabilities.includes(
    actions.find((a) => a.id === action)!.capability,
  );
