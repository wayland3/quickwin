// 剪贴板面板的纯函数与共享类型
//
// 只放可单测逻辑：相对时间、筛选枚举、键盘映射、选中落点、徽标规则。
// 组件与 IPC 放在 windows/ 与 lib/ipc.ts。

export type ClipKind = 'text' | 'html' | 'rtf' | 'image' | 'files';

export type ClipFilter = 'all' | 'text' | 'image' | 'files' | 'favorites';

export interface ClipboardItem {
  id: number;
  kind: ClipKind;
  preview: string;
  source_app: string | null;
  created_at: number;
  last_copied_at: number;
  copy_count: number;
  pinned: boolean;
  sensitive: boolean;
  truncated: boolean;
  byte_size: number;
  thumb_path: string | null;
}

export interface ClipboardDetail {
  id: number;
  kind: ClipKind;
  preview: string;
  text_body: string | null;
  html: string | null;
  rtf: string | null;
  file_list: string[] | null;
  file_exists: boolean[] | null;
  blob_path: string | null;
  thumb_path: string | null;
  byte_size: number;
  created_at: number;
  last_copied_at: number;
  copy_count: number;
  pinned: boolean;
  truncated: boolean;
  source_app: string | null;
}

export interface ClipboardListResult {
  items: ClipboardItem[];
  total: number;
}

export const FILTERS: ReadonlyArray<{ id: ClipFilter; label: string }> = [
  { id: 'all', label: '全部' },
  { id: 'text', label: '文本' },
  { id: 'image', label: '图片' },
  { id: 'files', label: '文件' },
  { id: 'favorites', label: '收藏' },
];

/** 循环切换筛选：`Tab` 向后、`Shift+Tab` 向前 */
export function cycleFilter(current: ClipFilter, direction: 1 | -1): ClipFilter {
  const index = FILTERS.findIndex((filter) => filter.id === current);
  const start = index < 0 ? 0 : index;
  const next = (start + direction + FILTERS.length) % FILTERS.length;
  return FILTERS[next]?.id ?? current;
}

/** 类型图标（缩略图恒为空时的占位） */
export function kindIcon(kind: ClipKind): string {
  switch (kind) {
    case 'html':
    case 'rtf':
      return '📝';
    case 'image':
      return '🖼';
    case 'files':
      return '📁';
    default:
      return '📄';
  }
}

/** 复制次数徽标：首次复制不显示，避免噪音 */
export function shouldShowCopyBadge(copyCount: number): boolean {
  return copyCount >= 2;
}

/** 数字千分位（列表头部的「1,204 条」） */
export function formatCount(value: number): string {
  return value.toLocaleString('zh-CN');
}

function pad2(value: number): string {
  return value.toString().padStart(2, '0');
}

/**
 * 相对时间（UI-SPEC §4.3）
 * - <1 分钟 刚刚
 * - <1 小时 N 分钟前
 * - <24 小时 N 小时前
 * - 昨天 昨天 HH:mm
 * - 今年内 MM-DD HH:mm
 * - 跨年 YYYY-MM-DD
 */
export function formatRelativeTime(timestamp: number, now = Date.now()): string {
  const diff = Math.max(0, now - timestamp);
  const minute = 60_000;
  const hour = 60 * minute;

  if (diff < minute) return '刚刚';
  if (diff < hour) return `${Math.floor(diff / minute)} 分钟前`;
  if (diff < 24 * hour) return `${Math.floor(diff / hour)} 小时前`;

  const date = new Date(timestamp);
  const nowDate = new Date(now);

  const yesterday = new Date(
    nowDate.getFullYear(),
    nowDate.getMonth(),
    nowDate.getDate() - 1,
  );
  if (
    date.getFullYear() === yesterday.getFullYear() &&
    date.getMonth() === yesterday.getMonth() &&
    date.getDate() === yesterday.getDate()
  ) {
    return `昨天 ${pad2(date.getHours())}:${pad2(date.getMinutes())}`;
  }

  if (date.getFullYear() === nowDate.getFullYear()) {
    return `${pad2(date.getMonth() + 1)}-${pad2(date.getDate())} ${pad2(date.getHours())}:${pad2(date.getMinutes())}`;
  }

  return `${date.getFullYear()}-${pad2(date.getMonth() + 1)}-${pad2(date.getDate())}`;
}

/** 完整时间（预览区底部） */
export function formatFullTime(timestamp: number): string {
  const date = new Date(timestamp);
  return `${date.getFullYear()}-${pad2(date.getMonth() + 1)}-${pad2(date.getDate())} ${pad2(date.getHours())}:${pad2(date.getMinutes())}:${pad2(date.getSeconds())}`;
}

/** 字节数：B / KB / MB */
export function formatBytes(bytes: number): string {
  if (bytes < 1024) return `${bytes} B`;
  if (bytes < 1024 * 1024) return `${(bytes / 1024).toFixed(1)} KB`;
  return `${(bytes / (1024 * 1024)).toFixed(1)} MB`;
}

export type ClipboardAction =
  | 'up'
  | 'down'
  | 'page-up'
  | 'page-down'
  | 'paste'
  | 'copy'
  | 'pin'
  | 'delete'
  | 'hard-delete'
  | 'filter-next'
  | 'filter-prev'
  | 'escape'
  | 'none';

export interface KeyInput {
  key: string;
  ctrlKey: boolean;
  shiftKey: boolean;
  metaKey: boolean;
  altKey: boolean;
  isComposing: boolean;
}

/**
 * 键盘映射（UI-SPEC §5）
 *
 * 输入法组合期间返回 `none`，方向键/回车/Tab/Delete 全部不响应。
 */
export function mapClipboardKey(input: KeyInput): ClipboardAction {
  if (input.isComposing) return 'none';

  if (input.key === 'Tab') {
    return input.shiftKey ? 'filter-prev' : 'filter-next';
  }

  if (input.key === 'Enter') {
    return input.ctrlKey ? 'copy' : 'paste';
  }

  if (input.key === 'Delete') {
    return input.shiftKey ? 'hard-delete' : 'delete';
  }

  if (input.key === 'Escape') return 'escape';

  if (input.ctrlKey && (input.key === 'p' || input.key === 'P')) return 'pin';

  switch (input.key) {
    case 'ArrowUp':
      return 'up';
    case 'ArrowDown':
      return 'down';
    case 'PageUp':
      return 'page-up';
    case 'PageDown':
      return 'page-down';
    default:
      return 'none';
  }
}

/** 选中移动：边界停住，不循环 */
export function moveSelection(
  index: number,
  delta: number,
  length: number,
): number {
  if (length <= 0) return -1;
  const base = index < 0 ? 0 : index;
  return Math.min(length - 1, Math.max(0, base + delta));
}

/**
 * 删除第 `index` 项后应选中的位置（删除前计算）
 *
 * - 优先下一项；删除的是末项则回退一项；清空后返回 -1
 */
export function selectionAfterDelete(index: number, length: number): number {
  if (length <= 1) return -1;
  if (index >= length - 1) return length - 2;
  return index;
}

/** 置顶分组与非置顶分组之间是否需要分隔线；返回非置顶首项的下标，无分隔线为 -1 */
export function dividerBeforeIndex(items: ReadonlyArray<{ pinned: boolean }>): number {
  const firstNonPinned = items.findIndex((item) => !item.pinned);
  if (firstNonPinned <= 0) return -1;
  return firstNonPinned;
}

/** 图片能否直接在 WebView 里解码（DIB 不能，避免必然失败的 `<img>` 请求） */
export function isPreviewableImage(blobPath: string | null | undefined): boolean {
  if (!blobPath) return false;
  return !/\.dib$/i.test(blobPath);
}

/** 文件名（预览区的文件列表） */
export function baseName(path: string): string {
  const parts = path.split(/[\\/]/);
  return parts[parts.length - 1] || path;
}

/** 目录（预览区的文件列表） */
export function dirName(path: string): string {
  const parts = path.split(/[\\/]/);
  parts.pop();
  return parts.join('\\');
}
