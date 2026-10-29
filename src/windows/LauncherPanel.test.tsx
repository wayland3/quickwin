import { beforeEach, describe, expect, it, vi } from 'vitest';
import { fireEvent, render, screen, waitFor } from '@testing-library/react';
import { LauncherPanel } from './LauncherPanel';
import { ipc } from '../lib/ipc';
import type { LauncherItem, LauncherQueryResult } from '../lib/launcher';

vi.mock('../lib/ipc', () => ({
  ipc: {
    launcherQuery: vi.fn(),
  },
}));

const results: LauncherItem[] = [
  {
    key: 'chrome.exe',
    title: 'Google Chrome',
    subtitle: 'C:\\Apps\\chrome.exe',
    kind: 'app',
    icon: null,
    score: 0.9,
  },
  {
    key: 'web:google:chr',
    title: '用 Google 搜索 "chr"',
    subtitle: '网页',
    kind: 'web',
    icon: null,
    score: 0,
  },
];

function mockResult(result: LauncherQueryResult) {
  vi.mocked(ipc.launcherQuery).mockResolvedValue(result);
}

describe('启动器面板', () => {
  beforeEach(() => {
    vi.clearAllMocks();
    mockResult({ items: results, hint: null });
  });

  it('展示分组与结果，键盘移动后 Enter 给出提示', async () => {
    render(<LauncherPanel />);

    await waitFor(() => expect(screen.getByText('Google Chrome')).toBeTruthy());
    expect(screen.getByText('应用')).toBeTruthy();
    expect(screen.getAllByText('网页').length).toBeGreaterThan(0);

    const input = screen.getByLabelText('搜索应用');
    fireEvent.keyDown(input, { key: 'ArrowDown' });
    const options = screen.getAllByRole('option');
    expect(options[1]?.getAttribute('aria-selected')).toBe('true');

    fireEvent.keyDown(input, { key: 'Enter' });
    await waitFor(() =>
      expect(screen.getByText('网页搜索将在 T-305 接入')).toBeTruthy(),
    );
  });

  it('查询失败时在底部提示且不崩溃', async () => {
    vi.mocked(ipc.launcherQuery).mockRejectedValue(new Error('数据库操作失败'));
    render(<LauncherPanel />);

    await waitFor(() => expect(screen.getByText('数据库操作失败')).toBeTruthy());
  });
});
