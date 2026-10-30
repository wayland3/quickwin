import { beforeEach, describe, expect, it, vi } from 'vitest';
import { fireEvent, render, screen, waitFor } from '@testing-library/react';
import { LauncherPanel } from './LauncherPanel';
import { ipc } from '../lib/ipc';
import type { LauncherItem, LauncherQueryResult } from '../lib/launcher';

vi.mock('../lib/ipc', () => ({
  ipc: {
    launcherQuery: vi.fn(),
    launcherExecute: vi.fn(),
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
    key: 'canary.exe',
    title: 'Chrome Canary',
    subtitle: 'C:\\Apps\\canary.exe',
    kind: 'app',
    icon: null,
    score: 0.8,
  },
];

function mockResult(result: LauncherQueryResult) {
  vi.mocked(ipc.launcherQuery).mockResolvedValue(result);
}

describe('启动器面板', () => {
  beforeEach(() => {
    vi.clearAllMocks();
    mockResult({ items: results, hint: null });
    vi.mocked(ipc.launcherExecute).mockResolvedValue(undefined);
  });

  it('展示分组与结果，键盘移动后选中第二项', async () => {
    render(<LauncherPanel />);

    await waitFor(() => expect(screen.getByText('Google Chrome')).toBeTruthy());
    expect(screen.getByText('应用')).toBeTruthy();
    expect(screen.getByText('Chrome Canary')).toBeTruthy();

    const input = screen.getByLabelText('搜索应用');
    fireEvent.keyDown(input, { key: 'ArrowDown' });
    const options = screen.getAllByRole('option');
    expect(options[1]?.getAttribute('aria-selected')).toBe('true');
  });

  it('查询失败时在底部提示且不崩溃', async () => {
    vi.mocked(ipc.launcherQuery).mockRejectedValue(new Error('数据库操作失败'));
    render(<LauncherPanel />);

    await waitFor(() => expect(screen.getByText('数据库操作失败')).toBeTruthy());
  });

  it('Enter 打开选中应用并调用 launcher_execute', async () => {
    render(<LauncherPanel />);

    await waitFor(() => expect(screen.getByText('Google Chrome')).toBeTruthy());
    fireEvent.keyDown(screen.getByLabelText('搜索应用'), { key: 'Enter' });

    await waitFor(() =>
      expect(ipc.launcherExecute).toHaveBeenCalledWith('chrome.exe'),
    );
  });

  it('打开失败时底部显示错误且面板保留', async () => {
    vi.mocked(ipc.launcherExecute).mockRejectedValue(
      new Error('无法打开该应用，请确认它仍然安装'),
    );
    render(<LauncherPanel />);

    await waitFor(() => expect(screen.getByText('Google Chrome')).toBeTruthy());
    fireEvent.keyDown(screen.getByLabelText('搜索应用'), { key: 'Enter' });

    await waitFor(() =>
      expect(screen.getByText('无法打开该应用，请确认它仍然安装')).toBeTruthy(),
    );
    expect(screen.getByLabelText('搜索应用')).toBeTruthy();
  });

  it('执行中重复按 Enter 只触发一次打开', async () => {
    let resolveLaunch: () => void = () => undefined;
    vi.mocked(ipc.launcherExecute).mockReturnValue(
      new Promise<void>((resolve) => {
        resolveLaunch = resolve;
      }),
    );
    render(<LauncherPanel />);

    await waitFor(() => expect(screen.getByText('Google Chrome')).toBeTruthy());
    const input = screen.getByLabelText('搜索应用');
    fireEvent.keyDown(input, { key: 'Enter' });
    fireEvent.keyDown(input, { key: 'Enter' });

    expect(ipc.launcherExecute).toHaveBeenCalledTimes(1);
    resolveLaunch();
  });
});
