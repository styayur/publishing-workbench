export interface Content {
  article_id: string;
  format: 'markdown' | 'mdx';
  source_path: string | null;
  title: string;
  slug: string;
  summary: string;
  body: string;
  cover: string | null;
  authors: string[];
  tags: string[];
  metadata: Record<string, unknown>;
}
export type Action = 'createDraft' | 'publish' | 'update' | 'delete';
export interface FieldSchema {
  type: string;
  title: string;
  placeholder?: string;
  secret?: boolean;
  multiline?: boolean;
  enum?: string[];
  default?: string;
}
export interface Manifest {
  id: string;
  name: string;
  version: string;
  description: string;
  capabilities: string[];
  schema: { properties: Record<string, FieldSchema>; required: string[] };
}
export interface Receipt {
  id: string;
  url: string | null;
  status: string;
  revision?: string | null;
}
export interface ApiError {
  code: string;
  message: string;
}
export interface History {
  target: string;
  action: Action;
  receipt: Receipt | null;
  error: ApiError | null;
  timestamp: number;
}
export interface Workspace {
  content: Content;
  asset_directory: string;
  extensions: Record<string, Record<string, string>>;
  history: History[];
}
export interface Prepared {
  content: Content;
  html: string;
  warnings: string[];
}
export const sample: Content = {
  article_id: crypto.randomUUID(),
  format: 'markdown',
  source_path: null,
  title: '一篇文章，多个目的地',
  slug: 'one-story-many-destinations',
  summary: '用统一内容模型，把创作与发布平台分开。',
  body: '# 写作从这里开始\n\n这是一篇 **Markdown** 文章。你可以编辑，也可以导入本地 Markdown 或 JSON。\n\n## 发布前检查\n\n- 填写标题和摘要\n- 为公众号添加封面\n- 配置扩展并测试连接\n\n> 默认先创建草稿，再由你决定何时发布。',
  cover: null,
  authors: [],
  tags: ['writing'],
  metadata: {},
};
export interface PublishJob {
  job_id: string;
  article_id: string;
  target_id: string;
  extension_id: string;
  operation: string;
  action: Action;
  status: string;
  started_at: number;
  completed_at: number | null;
  content: Content;
  steps: { name: string; status: string; detail: string }[];
  receipt: Receipt | null;
  error: ApiError | null;
  assets_reused: number;
  assets_uploaded: number;
  attempts: number;
}
export interface PublicationState {
  jobs: PublishJob[];
  mappings: {
    article_id: string;
    target_id: string;
    remote_id: string;
    remote_revision: string | null;
    last_published_hash: string;
    status: string;
  }[];
  plans: {
    extension_id: string;
    target_id: string;
    operation: string;
    remote_id: string | null;
  }[];
  orphans: {
    asset_hash: string;
    target_id: string;
    variant: string;
    remote_asset_id: string;
    remote_url: string;
    status: string;
  }[];
}
export interface ConfigSuggestion {
  extension_id: string;
  label: string;
  config: Record<string, string>;
  evidence: string;
}
