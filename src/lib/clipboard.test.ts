import { describe, expect, it } from 'vitest';
import {
  baseName,
  cycleFilter,
  dirName,
  dividerBeforeIndex,
  FILTERS,
  formatBytes,
  formatCount,
  formatRelativeTime,
  isPreviewableImage,
  kindIcon,
  mapClipboardKey,
  moveSelection,
  selectionAfterDelete,
  shouldShowCopyBadge,
} from './clipboard';

const NOW = new Date(2026, 9, 28, 12, 0, 0).getTime();
const MINUTE = 60_000;
const HOUR = 60 * MINUTE;

function key(input: Partial<Parameters<typeof mapClipboardKey>[0]>) {
  return mapClipboardKey({
    key: '',
    ctrlKey: false,
    shiftKey: false,
    metaKey: false,
    altKey: false,
    isComposing: false,
    ...input,
  });
}

describe('相对时间', () => {
  it('一分钟内显示刚刚', () => {
    expect(formatRelativeTime(NOW - 30_000, NOW)).toBe('刚刚');
  });

  it('一小时内显示分钟', () => {
    expect(formatRelativeTime(NOW - 5 * MINUTE, NOW)).toBe('5 分钟前');
  });

  it('24 小时内显示小时', () => {
    expect(formatRelativeTime(NOW - 3 * HOUR, NOW)).toBe('3 小时前');
  });

  it('昨天显示日期与时刻', () => {
    const yesterdayMorning = new Date(2026, 9, 27, 8, 5, 0).getTime();
    expect(formatRelativeTime(yesterdayMorning, NOW)).toBe('昨天 08:05');
  });

  it('今年内显示月日与时刻', () => {
    const earlier = new Date(2026, 9, 20, 14, 2, 0).getTime();
    expect(formatRelativeTime(earlier, NOW)).toBe('10-20 14:02');
  });

  it('跨年只显示日期', () => {
    const lastYear = new Date(2025, 11, 31, 23, 0, 0).getTime();
    expect(formatRelativeTime(lastYear, NOW)).toBe('2025-12-31');
  });

  it('未来时间按刚刚处理', () => {
    expect(formatRelativeTime(NOW + 10_000, NOW)).toBe('刚刚');
  });
});

describe('筛选循环', () => {
  it('向后循环到末尾再回到开头', () => {
    let current = FILTERS[0]?.id ?? 'all';
    const seen: string[] = [];
    for (let step = 0; step < FILTERS.length; step += 1) {
      current = cycleFilter(current, 1);
      seen.push(current);
    }
    expect(seen).toEqual(['text', 'image', 'files', 'favorites', 'all']);
  });

  it('向前循环方向相反', () => {
    expect(cycleFilter('all', -1)).toBe('favorites');
    expect(cycleFilter('text', -1)).toBe('all');
  });
});

describe('键盘映射', () => {
  it('方向键与翻页', () => {
    expect(key({ key: 'ArrowUp' })).toBe('up');
    expect(key({ key: 'ArrowDown' })).toBe('down');
    expect(key({ key: 'PageUp' })).toBe('page-up');
    expect(key({ key: 'PageDown' })).toBe('page-down');
  });

  it('回车与仅复制', () => {
    expect(key({ key: 'Enter' })).toBe('paste');
    expect(key({ key: 'Enter', ctrlKey: true })).toBe('copy');
  });

  it('置顶与删除', () => {
    expect(key({ key: 'p', ctrlKey: true })).toBe('pin');
    expect(key({ key: 'P', ctrlKey: true })).toBe('pin');
    expect(key({ key: 'Delete' })).toBe('delete');
    expect(key({ key: 'Delete', shiftKey: true })).toBe('hard-delete');
  });

  it('Tab 切换筛选', () => {
    expect(key({ key: 'Tab' })).toBe('filter-next');
    expect(key({ key: 'Tab', shiftKey: true })).toBe('filter-prev');
  });

  it('Esc 关闭', () => {
    expect(key({ key: 'Escape' })).toBe('escape');
  });

  it('输入法组合期间全部不响应', () => {
    for (const input of [
      { key: 'ArrowDown' },
      { key: 'Enter' },
      { key: 'Tab' },
      { key: 'Delete' },
      { key: 'p', ctrlKey: true },
    ]) {
      expect(key({ ...input, isComposing: true })).toBe('none');
    }
  });

  it('未绑定按键不响应', () => {
    expect(key({ key: 'a' })).toBe('none');
    expect(key({ key: 'F5' })).toBe('none');
  });
});

describe('选中移动与删除落点', () => {
  it('上下移动在边界停住', () => {
    expect(moveSelection(0, -1, 5)).toBe(0);
    expect(moveSelection(4, 1, 5)).toBe(4);
    expect(moveSelection(2, 1, 5)).toBe(3);
    expect(moveSelection(-1, 1, 5)).toBe(1);
  });

  it('空列表返回 -1', () => {
    expect(moveSelection(0, 1, 0)).toBe(-1);
    expect(selectionAfterDelete(0, 0)).toBe(-1);
  });

  it('删除中间项选中下一项', () => {
    expect(selectionAfterDelete(1, 3)).toBe(1);
  });

  it('删除末项回退一项', () => {
    expect(selectionAfterDelete(2, 3)).toBe(1);
  });

  it('删除最后一条清空选中', () => {
    expect(selectionAfterDelete(0, 1)).toBe(-1);
  });
});

describe('置顶分隔线', () => {
  it('有置顶也有非置顶时返回非置顶首项下标', () => {
    expect(dividerBeforeIndex([{ pinned: true }, { pinned: true }, { pinned: false }])).toBe(2);
  });

  it('全部置顶或全部非置顶不画线', () => {
    expect(dividerBeforeIndex([{ pinned: true }])).toBe(-1);
    expect(dividerBeforeIndex([{ pinned: false }, { pinned: false }])).toBe(-1);
    expect(dividerBeforeIndex([])).toBe(-1);
  });
});

describe('展示辅助', () => {
  it('复制次数徽标仅在两次以上显示', () => {
    expect(shouldShowCopyBadge(1)).toBe(false);
    expect(shouldShowCopyBadge(2)).toBe(true);
    expect(shouldShowCopyBadge(9)).toBe(true);
  });

  it('类型图标', () => {
    expect(kindIcon('text')).toBe('📄');
    expect(kindIcon('html')).toBe('📝');
    expect(kindIcon('image')).toBe('🖼');
    expect(kindIcon('files')).toBe('📁');
  });

  it('DIB 不渲染图片', () => {
    expect(isPreviewableImage('C:\\data\\a.png')).toBe(true);
    expect(isPreviewableImage('C:\\data\\a.dib')).toBe(false);
    expect(isPreviewableImage(null)).toBe(false);
  });

  it('文件路径拆分', () => {
    expect(baseName('C:\\Users\\a\\report.pdf')).toBe('report.pdf');
    expect(dirName('C:\\Users\\a\\report.pdf')).toBe('C:\\Users\\a');
  });

  it('字节数与计数格式化', () => {
    expect(formatBytes(512)).toBe('512 B');
    expect(formatBytes(2048)).toBe('2.0 KB');
    expect(formatBytes(3 * 1024 * 1024)).toBe('3.0 MB');
    expect(formatCount(1204)).toBe('1,204');
  });
});
