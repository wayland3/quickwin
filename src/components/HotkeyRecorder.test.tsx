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

  it('Win 键导致失焦后仍能录到 Win+空格', async () => {
    const onSave = vi.fn().mockResolvedValue(undefined);
    render(<HotkeyRecorder value="Alt+Space" action="launcher" onSave={onSave} />);

    const button = screen.getByText('Alt+Space');
    fireEvent.click(button);

    await waitFor(() => {
      expect(screen.getByText('按下新热键...')).toBeDefined();
    });

    // 1. 按下 Win 键：系统唤起开始菜单 / 桌面 Activities
    fireEvent.keyDown(button, { key: 'Meta', metaKey: true });
    // 2. 窗口失焦
    fireEvent.blur(button);

    // 失焦后仍保持录制态（修饰键还按着）
    expect(screen.getByText('按下新热键...')).toBeDefined();

    // 3. 按下空格，Win 仍按住
    fireEvent.keyDown(button, { key: ' ', metaKey: true });

    await waitFor(() => {
      expect(onSave).toHaveBeenCalledWith('Super+Space');
    });
  });

  it('修饰键松开后未录到组合键则退出录制态', async () => {
    const onSave = vi.fn().mockResolvedValue(undefined);
    render(<HotkeyRecorder value="Alt+Space" action="launcher" onSave={onSave} />);

    const button = screen.getByText('Alt+Space');
    fireEvent.click(button);

    await waitFor(() => {
      expect(screen.getByText('按下新热键...')).toBeDefined();
    });

    // 按下 Win 键后失焦，再松开 Win 键（没有按主键）
    fireEvent.keyDown(button, { key: 'Meta', metaKey: true });
    fireEvent.blur(button);
    fireEvent.keyUp(button, { key: 'Meta', metaKey: false });

    await waitFor(() => {
      expect(screen.getByText('Alt+Space')).toBeDefined();
    });
    expect(onSave).not.toHaveBeenCalled();
  });

  it('无修饰键时失焦仍退出录制态', async () => {
    const onSave = vi.fn().mockResolvedValue(undefined);
    render(<HotkeyRecorder value="Alt+Space" action="launcher" onSave={onSave} />);

    const button = screen.getByText('Alt+Space');
    fireEvent.click(button);

    await waitFor(() => {
      expect(screen.getByText('按下新热键...')).toBeDefined();
    });

    fireEvent.blur(button);

    await waitFor(() => {
      expect(screen.getByText('Alt+Space')).toBeDefined();
    });
    expect(onSave).not.toHaveBeenCalled();
  });

  it('保存失败后松开修饰键退出录制态并保留错误提示', async () => {
    // 后端拒绝保存（例如与其他热键重复）
    const onSave = vi.fn().mockRejectedValue(new Error('两个全局热键不能重复'));
    render(<HotkeyRecorder value="Alt+Space" action="launcher" onSave={onSave} />);

    const button = screen.getByText('Alt+Space');
    fireEvent.click(button);

    await waitFor(() => {
      expect(screen.getByText('按下新热键...')).toBeDefined();
    });

    // 按 Win 键后失焦
    fireEvent.keyDown(button, { key: 'Meta', metaKey: true });
    fireEvent.blur(button);

    // 再按空格，触发一次失败的保存
    fireEvent.keyDown(button, { key: ' ', metaKey: true });

    await waitFor(() => {
      expect(onSave).toHaveBeenCalledWith('Super+Space');
    });
    // 保存失败，错误提示可见，仍在录制态（显示已录到的草稿）
    expect(screen.getByText('两个全局热键不能重复')).toBeDefined();
    expect(screen.getByText('Super+Space')).toBeDefined();

    // 松开 Win 键 → 退出录制态，错误提示仍保留
    fireEvent.keyUp(button, { key: 'Meta', metaKey: false });

    await waitFor(() => {
      expect(screen.getByText('Alt+Space')).toBeDefined();
    });
    expect(screen.getByText('两个全局热键不能重复')).toBeDefined();
  });
});
