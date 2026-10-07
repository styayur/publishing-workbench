import { invoke, isTauri } from '@tauri-apps/api/core';
export const desktop = isTauri();
export async function api<T>(
  command: string,
  args?: Record<string, unknown>,
): Promise<T> {
  if (!desktop)
    throw {
      code: 'desktop_required',
      message:
        '浏览器只提供编辑预览；真实发布与凭据管理请运行 npm run tauri dev',
    };
  return invoke<T>(command, args);
}
export function message(error: unknown): string {
  if (typeof error === 'object' && error && 'message' in error)
    return String(error.message);
  return typeof error === 'string' ? error : '操作失败，请重试';
}
