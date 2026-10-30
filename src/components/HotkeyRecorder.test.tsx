import { fireEvent, render, screen, waitFor } from '@testing-library/react';
import { describe, expect, it, vi } from 'vitest';
import { HotkeyRecorder } from './HotkeyRecorder';

// Mock ipc
vi.mock('../lib/ipc', () => ({
  ipc: {
    hotkeySuspend: vi.fn().mockResolvedValue(undefined),
    hotkeyResume: vi.fn().mockResolvedValue(undefined),
  },
}));

describe('HotkeyRecorder', () => {
  it('显示当前热键值', () => {
    const onSave = vi.fn();
    render(<HotkeyRecorder value="Alt+Space" action="launcher" onSave={onSave} />);
    expect(screen.getByText('Alt+Space')).toBeDefined();
  });

  it('无值时显示占位符', () => {
    const onSave = vi.fn();
    render(
      <HotkeyRecorder value="" action="launcher" onSave={onSave} placeholder="点击录制" />,
    );
    expect(screen.getByText('点击录制')).toBeDefined();
  });

  it('点击后进入录制态', async () => {
    const onSave = vi.fn();
    render(<HotkeyRecorder value="Alt+Space" action="launcher" onSave={onSave} />);

    const button = screen.getByText('Alt+Space');
    fireEvent.click(button);

    await waitFor(() => {
      expect(screen.getByText('按下新热键...')).toBeDefined();
    });
  });

  it('按 Esc 取消录制', async () => {
    const onSave = vi.fn();
    render(<HotkeyRecorder value="Alt+Space" action="launcher" onSave={onSave} />);

    const button = screen.getByText('Alt+Space');
    fireEvent.click(button);

    await waitFor(() => {
      expect(screen.getByText('按下新热键...')).toBeDefined();
    });

    fireEvent.keyDown(button, { key: 'Escape' });

    await waitFor(() => {
      expect(screen.getByText('Alt+Space')).toBeDefined();
    });

    expect(onSave).not.toHaveBeenCalled();
  });

  it('按 Backspace 清空热键', async () => {
    const onSave = vi.fn().mockResolvedValue(undefined);
    render(<HotkeyRecorder value="Alt+Space" action="launcher" onSave={onSave} />);

    const button = screen.getByText('Alt+Space');
    fireEvent.click(button);

    await waitFor(() => {
      expect(screen.getByText('按下新热键...')).toBeDefined();
    });

    fireEvent.keyDown(button, { key: 'Backspace' });

    await waitFor(() => {
      expect(onSave).toHaveBeenCalledWith('');
    });
  });

  it('录制组合键后自动保存', async () => {
    const onSave = vi.fn().mockResolvedValue(undefined);
    render(<HotkeyRecorder value="Alt+Space" action="launcher" onSave={onSave} />);

    const button = screen.getByText('Alt+Space');
    fireEvent.click(button);

    await waitFor(() => {
      expect(screen.getByText('按下新热键...')).toBeDefined();
    });

    // 模拟按下 Ctrl+K
    fireEvent.keyDown(button, { key: 'k', ctrlKey: true });

    await waitFor(() => {
      expect(onSave).toHaveBeenCalledWith('Ctrl+K');
    });
  });

  it('没有修饰键时不保存', async () => {
    const onSave = vi.fn();
    render(<HotkeyRecorder value="Alt+Space" action="launcher" onSave={onSave} />);

    const button = screen.getByText('Alt+Space');
    fireEvent.click(button);

    await waitFor(() => {
      expect(screen.getByText('按下新热键...')).toBeDefined();
    });

    // 模拟只按 K（无修饰键）
    fireEvent.keyDown(button, { key: 'k' });

    // 应该保持录制态
    expect(screen.getByText('按下新热键...')).toBeDefined();
    expect(onSave).not.toHaveBeenCalled();
  });
});
