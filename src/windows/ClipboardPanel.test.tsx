import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import { cleanup, fireEvent, render, screen, waitFor } from '@testing-library/react';
import { ClipboardPanel } from './ClipboardPanel';
import { ipc } from '../lib/ipc';
import type { ClipboardDetail, ClipboardItem, ClipboardListResult } from '../lib/clipboard';

vi.mock('@tauri-apps/api/event', () => ({
  listen: vi.fn(() => Promise.resolve(() => undefined)),
}));

vi.mock('@tauri-apps/api/core', () => ({
  convertFileSrc: vi.fn((path: string) => `asset://${path}`),
}));

vi.mock('../lib/ipc', () => ({
  ipc: {
    clipboardList: vi.fn(),
    clipboardGet: vi.fn(),
    clipboardSetPinned: vi.fn(),
    clipboardDelete: vi.fn(),
    clipboardActivate: vi.fn(),
    overlayHide: vi.fn(),
  },
}));

function item(overrides: Partial<ClipboardItem> = {}): ClipboardItem {
  return {
    id: 1,
    kind: 'text',
    preview: '示例文本',
    source_app: 'notepad.exe',
    created_at: Date.now(),
    last_copied_at: Date.now(),
    copy_count: 1,
    pinned: false,
    sensitive: false,
    truncated: false,
    byte_size: 12,
    thumb_path: null,
    ...overrides,
  };
}

function detail(id: number): ClipboardDetail {
  return {
    id,
    kind: 'text',
    preview: '示例文本',
    text_body: '示例文本正文',
    html: null,
    rtf: null,
    file_list: null,
    file_exists: null,
    blob_path: null,
    thumb_path: null,
    byte_size: 12,
    created_at: Date.now(),
    last_copied_at: Date.now(),
    copy_count: 1,
    pinned: false,
    truncated: false,
    source_app: 'notepad.exe',
  };
}

function listResult(items: ClipboardItem[], total = items.length): ClipboardListResult {
  return { items, total };
}

beforeEach(() => {
  vi.mocked(ipc.clipboardList).mockReset();
  vi.mocked(ipc.clipboardGet).mockReset().mockResolvedValue(detail(1));
  vi.mocked(ipc.clipboardSetPinned).mockReset().mockResolvedValue(undefined);
  vi.mocked(ipc.clipboardDelete).mockReset().mockResolvedValue(undefined);
  vi.mocked(ipc.clipboardActivate).mockReset().mockResolvedValue(undefined);
  vi.mocked(ipc.overlayHide).mockReset().mockResolvedValue(undefined);
});

afterEach(() => {
  cleanup();
});

describe('剪贴板面板', () => {
  it('首屏加载后渲染列表', async () => {
    vi.mocked(ipc.clipboardList).mockResolvedValue(listResult([item()]));

    render(<ClipboardPanel />);

    expect(screen.getByRole('listbox')).toBeTruthy();
    await waitFor(() => {
      expect(screen.getByText('示例文本')).toBeTruthy();
    });
    expect(screen.getAllByText('1 条').length).toBeGreaterThan(0);
  });

  it('空历史显示空态文案', async () => {
    vi.mocked(ipc.clipboardList).mockResolvedValue(listResult([]));

    render(<ClipboardPanel />);

    await waitFor(() => {
      expect(screen.getByText('还没有记录')).toBeTruthy();
    });
  });

  it('无搜索结果提示当前关键词', async () => {
    vi.mocked(ipc.clipboardList).mockResolvedValue(listResult([]));

    render(<ClipboardPanel />);
    await waitFor(() => expect(screen.getByText('还没有记录')).toBeTruthy());

    fireEvent.change(screen.getByLabelText('搜索剪贴板'), {
      target: { value: '找不到的词' },
    });

    await waitFor(() => {
      expect(screen.getByText('没有找到 “找不到的词”')).toBeTruthy();
    });
  });

  it('连续输入只触发一次查询（80ms 防抖）', async () => {
    vi.mocked(ipc.clipboardList).mockResolvedValue(listResult([item()]));

    render(<ClipboardPanel />);
    await waitFor(() => expect(ipc.clipboardList).toHaveBeenCalled());
    const baseline = vi.mocked(ipc.clipboardList).mock.calls.length;

    const input = screen.getByLabelText('搜索剪贴板');
    fireEvent.change(input, { target: { value: 'a' } });
    fireEvent.change(input, { target: { value: 'ab' } });
    fireEvent.change(input, { target: { value: 'abc' } });

    await waitFor(
      () => {
        expect(vi.mocked(ipc.clipboardList).mock.calls.length).toBe(baseline + 1);
      },
      { timeout: 1000 },
    );

    await new Promise((resolve) => window.setTimeout(resolve, 150));
    expect(vi.mocked(ipc.clipboardList).mock.calls.length).toBe(baseline + 1);
  });

  it('旧请求的过期响应不覆盖新结果', async () => {
    let resolveFirst: ((value: ClipboardListResult) => void) | undefined;

    vi.mocked(ipc.clipboardList)
      .mockImplementationOnce(
        () =>
          new Promise<ClipboardListResult>((resolve) => {
            resolveFirst = resolve;
          }),
      )
      .mockResolvedValue(listResult([item({ id: 2, preview: '新结果' })]));

    render(<ClipboardPanel />);

    // 等首屏请求真正发出（80ms 防抖），再改搜索词触发第二代请求
    await waitFor(() => expect(ipc.clipboardList).toHaveBeenCalledTimes(1));

    fireEvent.change(screen.getByLabelText('搜索剪贴板'), {
      target: { value: 'new' },
    });

    await waitFor(() => {
      expect(screen.getByText('新结果')).toBeTruthy();
    });

    // 旧请求晚到：必须被丢弃
    resolveFirst?.(listResult([item({ id: 1, preview: '旧结果' })]));
    await new Promise((resolve) => window.setTimeout(resolve, 20));

    expect(screen.queryByText('旧结果')).toBeNull();
    expect(screen.getByText('新结果')).toBeTruthy();
  });

  it('置顶项与非置顶项之间有分隔线', async () => {
    vi.mocked(ipc.clipboardList).mockResolvedValue(
      listResult([
        item({ id: 1, preview: '置顶项', pinned: true }),
        item({ id: 2, preview: '普通项' }),
      ]),
    );

    render(<ClipboardPanel />);

    await waitFor(() => expect(screen.getByText('普通项')).toBeTruthy());
    expect(screen.getByRole('separator')).toBeTruthy();
  });

  it('Ctrl+P 调用置顶接口', async () => {
    vi.mocked(ipc.clipboardList).mockResolvedValue(listResult([item()]));

    render(<ClipboardPanel />);
    await waitFor(() => expect(screen.getByText('示例文本')).toBeTruthy());

    fireEvent.keyDown(screen.getByLabelText('搜索剪贴板'), {
      key: 'p',
      ctrlKey: true,
    });

    await waitFor(() => {
      expect(ipc.clipboardSetPinned).toHaveBeenCalledWith(1, true);
    });
  });

  it('滚动到底部追加下一页并按 id 去重', async () => {
    const firstPage = Array.from({ length: 50 }, (_, index) =>
      item({ id: index + 1, preview: `条目${index + 1}` }),
    );
    vi.mocked(ipc.clipboardList)
      .mockResolvedValueOnce(listResult(firstPage, 51))
      .mockResolvedValueOnce(
        listResult([item({ id: 51, preview: '第 51 条' }), item({ id: 1, preview: '条目1' })], 51),
      );

    render(<ClipboardPanel />);
    await waitFor(() => expect(screen.getByText('条目1')).toBeTruthy());

    fireEvent.scroll(screen.getByRole('listbox'));

    await waitFor(() => expect(screen.getByText('第 51 条')).toBeTruthy());
    expect(vi.mocked(ipc.clipboardList).mock.calls[1]?.[0]).toMatchObject({ offset: 50 });
    // 重复 id 只保留一条
    expect(screen.getAllByText('条目1')).toHaveLength(1);

    // 已加载数达到 total 后不再请求
    const calls = vi.mocked(ipc.clipboardList).mock.calls.length;
    fireEvent.scroll(screen.getByRole('listbox'));
    await new Promise((resolve) => window.setTimeout(resolve, 20));
    expect(vi.mocked(ipc.clipboardList).mock.calls.length).toBe(calls);
  });

  it('新条件下旧首屏响应晚到时被丢弃', async () => {
    let resolveStale: ((value: ClipboardListResult) => void) | undefined;
    let resolvePage: ((value: ClipboardListResult) => void) | undefined;

    vi.mocked(ipc.clipboardList)
      .mockImplementationOnce(
        () =>
          new Promise<ClipboardListResult>((resolve) => {
            resolveStale = resolve;
          }),
      )
      .mockResolvedValueOnce(
        listResult([item({ id: 10, preview: '新条件首屏' })], 100),
      )
      .mockImplementationOnce(
        () =>
          new Promise<ClipboardListResult>((resolve) => {
            resolvePage = resolve;
          }),
      );

    render(<ClipboardPanel />);
    await waitFor(() => expect(ipc.clipboardList).toHaveBeenCalledTimes(1));

    fireEvent.change(screen.getByLabelText('搜索剪贴板'), {
      target: { value: '新条件' },
    });
    await waitFor(() => expect(screen.getByText('新条件首屏')).toBeTruthy());

    fireEvent.scroll(screen.getByRole('listbox'));
    await waitFor(() => expect(ipc.clipboardList).toHaveBeenCalledTimes(3));

    // 旧首屏晚到：必须整条丢弃，不能被当成翻页结果追加
    resolveStale?.(listResult([item({ id: 99, preview: '旧条件结果' })], 100));
    await new Promise((resolve) => window.setTimeout(resolve, 20));
    expect(screen.queryByText('旧条件结果')).toBeNull();

    resolvePage?.(listResult([item({ id: 11, preview: '翻页结果' })], 100));
    await waitFor(() => expect(screen.getByText('翻页结果')).toBeTruthy());
  });

  it('详情读取失败时提示条目已不存在并刷新', async () => {
    vi.mocked(ipc.clipboardList).mockResolvedValue(listResult([item()]));
    vi.mocked(ipc.clipboardGet).mockRejectedValue({
      code: 'storage.error',
      message: '数据库操作失败',
    });

    render(<ClipboardPanel />);

    await waitFor(() => {
      expect(screen.getByText('条目已不存在，列表已刷新')).toBeTruthy();
    });
  });

  it('方向键与 PgDn 移动选中，跨页时先加载', async () => {
    const firstPage = Array.from({ length: 5 }, (_, index) =>
      item({ id: index + 1, preview: `条目${index + 1}` }),
    );
    const secondPage = Array.from({ length: 5 }, (_, index) =>
      item({ id: index + 6, preview: `条目${index + 6}` }),
    );
    vi.mocked(ipc.clipboardList)
      .mockResolvedValueOnce(listResult(firstPage, 10))
      .mockResolvedValueOnce(listResult(secondPage, 10));

    render(<ClipboardPanel />);
    await waitFor(() => expect(screen.getByText('条目1')).toBeTruthy());

    const input = screen.getByLabelText('搜索剪贴板');
    const optionAt = (index: number) => screen.getAllByRole('option')[index];

    expect(optionAt(0)?.getAttribute('aria-selected')).toBe('true');

    fireEvent.keyDown(input, { key: 'ArrowDown' });
    await waitFor(() => {
      expect(optionAt(1)?.getAttribute('aria-selected')).toBe('true');
    });

    // 跨页：PgDn 先加载第二页再移动
    fireEvent.keyDown(input, { key: 'PageDown' });
    await waitFor(() => expect(ipc.clipboardList).toHaveBeenCalledTimes(2));
    expect(vi.mocked(ipc.clipboardList).mock.calls[1]?.[0]).toMatchObject({ offset: 5 });

    await waitFor(() => {
      expect(optionAt(9)?.getAttribute('aria-selected')).toBe('true');
    });
  });

  it('输入法组合期间不响应方向键', async () => {
    vi.mocked(ipc.clipboardList).mockResolvedValue(
      listResult([
        item({ id: 1, preview: '第一条' }),
        item({ id: 2, preview: '第二条' }),
      ]),
    );

    render(<ClipboardPanel />);
    await waitFor(() => expect(screen.getByText('第一条')).toBeTruthy());

    const input = screen.getByLabelText('搜索剪贴板');
    const selectedIndex = () =>
      screen.getAllByRole('option').findIndex((node) => node.getAttribute('aria-selected') === 'true');

    fireEvent.compositionStart(input);
    fireEvent.keyDown(input, { key: 'ArrowDown' });
    expect(selectedIndex()).toBe(0);

    fireEvent.compositionEnd(input);
    fireEvent.keyDown(input, { key: 'ArrowDown' });
    await waitFor(() => expect(selectedIndex()).toBe(1));
  });

  it('删除后选中落到下一项', async () => {
    const items = [
      item({ id: 1, preview: '第一条' }),
      item({ id: 2, preview: '第二条' }),
      item({ id: 3, preview: '第三条' }),
    ];
    vi.mocked(ipc.clipboardList)
      .mockResolvedValueOnce(listResult(items, 3))
      .mockResolvedValue(
        listResult([items[0]!, items[2]!], 2),
      );

    render(<ClipboardPanel />);
    await waitFor(() => expect(screen.getByText('第二条')).toBeTruthy());

    fireEvent.click(screen.getByText('第二条'));
    fireEvent.keyDown(screen.getByLabelText('搜索剪贴板'), { key: 'Delete' });

    await waitFor(() => expect(ipc.clipboardDelete).toHaveBeenCalledWith(2, false));
    await waitFor(() => {
      const selected = screen
        .getAllByRole('option')
        .find((node) => node.getAttribute('aria-selected') === 'true');
      expect(selected?.textContent).toContain('第三条');
    });
  });

  it('Enter 触发粘贴，Ctrl+Enter 触发仅复制', async () => {
    vi.mocked(ipc.clipboardList).mockResolvedValue(listResult([item({ id: 7 })]));

    render(<ClipboardPanel />);
    await waitFor(() => expect(screen.getByText('示例文本')).toBeTruthy());

    const input = screen.getByLabelText('搜索剪贴板');
    fireEvent.keyDown(input, { key: 'Enter' });
    await waitFor(() => expect(ipc.clipboardActivate).toHaveBeenCalledWith(7, 'paste'));

    fireEvent.keyDown(input, { key: 'Enter', ctrlKey: true });
    await waitFor(() => expect(ipc.clipboardActivate).toHaveBeenCalledWith(7, 'copy'));
  });
});
