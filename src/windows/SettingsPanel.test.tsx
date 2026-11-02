import { beforeEach, describe, expect, it, vi } from 'vitest';
import { act, fireEvent, render, screen, waitFor } from '@testing-library/react';
import { SettingsPanel } from './SettingsPanel';
import { ipc } from '../lib/ipc';
import type { Settings } from '../lib/clipboard';

vi.mock('../lib/ipc', () => ({
  ipc: {
    settingsGet: vi.fn(),
    settingsUpdate: vi.fn(),
    hotkeyProbe: vi.fn(),
    hotkeySuspend: vi.fn(),
    hotkeyResume: vi.fn(),
  },
}));

const settings: Settings = {
  launcher_hotkey: 'Super+Space',
  clipboard_hotkey: 'Alt+V',
  retention_days: 15,
  max_items: 10000,
  record_text: true,
  record_image: true,
  record_files: true,
  excluded_apps: [],
  theme: 'system',
  autostart: false,
  pause_recording: false,
  restore_clipboard_after_paste: false,
};

describe('设置窗口热键页', () => {
  beforeEach(() => {
    vi.clearAllMocks();
    vi.mocked(ipc.settingsGet).mockResolvedValue(settings);
    vi.mocked(ipc.hotkeyProbe).mockImplementation(async (accelerator: string) => ({
      accelerator,
      available: true,
      reason: 'available' as const,
      message: '当前可用',
    }));
  });

  async function openHotkeyTab() {
    render(<SettingsPanel />);
    await waitFor(() => {
      expect(screen.getByText('常规')).toBeDefined();
    });
    await act(async () => {
      fireEvent.click(screen.getByText('热键'));
    });
  }

  it('进入热键页后实测两个热键的可用性', async () => {
    await openHotkeyTab();

    await waitFor(() => {
      expect(ipc.hotkeyProbe).toHaveBeenCalledWith('Super+Space');
    });
    expect(ipc.hotkeyProbe).toHaveBeenCalledWith('Alt+V');
  });

  it('显示可用结论', async () => {
    await openHotkeyTab();

    await waitFor(() => {
      expect(screen.getAllByText('当前可用').length).toBe(2);
    });
  });

  it('显示被占用结论', async () => {
    vi.mocked(ipc.hotkeyProbe).mockImplementation(async (accelerator: string) => ({
      accelerator,
      available: false,
      reason: 'occupied' as const,
      message: `${accelerator} 已被系统或其他程序占用`,
    }));

    await openHotkeyTab();

    await waitFor(() => {
      expect(screen.getByText('Super+Space 已被系统或其他程序占用')).toBeDefined();
    });
  });

  it('探测失败时展示后端报错信息', async () => {
    vi.mocked(ipc.hotkeyProbe).mockRejectedValue(
      new Error('无法检测热键占用：无法连接 X11'),
    );

    await openHotkeyTab();

    await waitFor(() => {
      // 两个热键都探测失败，因此有两条相同提示
      expect(screen.getAllByText('无法检测热键占用：无法连接 X11').length).toBe(2);
    });
  });

  it('点击重新检测会再次实测', async () => {
    await openHotkeyTab();

    await waitFor(() => {
      expect(ipc.hotkeyProbe).toHaveBeenCalledTimes(2);
    });

    const buttons = screen.getAllByText('重新检测');
    await act(async () => {
      fireEvent.click(buttons[0]!);
    });

    await waitFor(() => {
      expect(ipc.hotkeyProbe).toHaveBeenCalledTimes(3);
    });
  });
});
