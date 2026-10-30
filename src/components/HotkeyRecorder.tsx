import { useCallback, useEffect, useRef, useState } from 'react';
import { eventToAccelerator } from '../lib/keys';
import { ipc } from '../lib/ipc';

interface HotkeyRecorderProps {
  /** 当前保存的热键值 */
  value: string;
  /** 热键动作标识，用于 suspend/resume */
  action: 'launcher' | 'clipboard';
  /** 保存回调 */
  onSave: (value: string) => Promise<void>;
  /** 占位文本 */
  placeholder?: string;
}

/**
 * 热键录制控件
 *
 * 点击按钮进入录制态，按下组合键自动保存。
 * - Esc：取消录制，恢复原值
 * - Backspace：清空热键
 * - 录制期间暂停全局热键，避免误触发
 */
export function HotkeyRecorder({
  value,
  action,
  onSave,
  placeholder = '点击录制',
}: HotkeyRecorderProps) {
  const [isRecording, setIsRecording] = useState(false);
  const [draft, setDraft] = useState<string | null>(null);
  const [error, setError] = useState<string | null>(null);
  const buttonRef = useRef<HTMLButtonElement>(null);

  // 进入录制态
  const startRecording = useCallback(async () => {
    setIsRecording(true);
    setDraft(null);
    setError(null);

    // 暂停全局热键，避免录制时误触发
    try {
      await ipc.hotkeySuspend(action);
    } catch {
      // 暂停失败不阻断录制，只是可能误触发
    }

    // 聚焦按钮以捕获键盘事件
    buttonRef.current?.focus();
  }, [action]);

  // 退出录制态
  const stopRecording = useCallback(async () => {
    setIsRecording(false);
    setDraft(null);

    // 恢复全局热键
    try {
      await ipc.hotkeyResume(action);
    } catch {
      // 恢复失败不影响 UI
    }
  }, [action]);

  // 保存新热键
  const saveHotkey = useCallback(
    async (newValue: string) => {
      try {
        await onSave(newValue);
        setError(null);
        await stopRecording();
      } catch (err) {
        setError(err instanceof Error ? err.message : String(err));
        // 保存失败，保持录制态让用户重试
      }
    },
    [onSave, stopRecording],
  );

  // 清空热键
  const clearHotkey = useCallback(async () => {
    try {
      await onSave('');
      setError(null);
      await stopRecording();
    } catch (err) {
      setError(err instanceof Error ? err.message : String(err));
    }
  }, [onSave, stopRecording]);

  // 键盘事件处理
  const handleKeyDown = useCallback(
    async (event: React.KeyboardEvent) => {
      if (!isRecording) return;

      event.preventDefault();
      event.stopPropagation();

      // Esc：取消录制
      if (event.key === 'Escape') {
        await stopRecording();
        return;
      }

      // Backspace：清空热键
      if (event.key === 'Backspace') {
        await clearHotkey();
        return;
      }

      // 尝试构建 accelerator
      const accelerator = eventToAccelerator(event.nativeEvent);
      if (accelerator) {
        setDraft(accelerator);
        await saveHotkey(accelerator);
      }
    },
    [isRecording, stopRecording, clearHotkey, saveHotkey],
  );

  // 失焦时退出录制态
  const handleBlur = useCallback(() => {
    if (isRecording) {
      stopRecording();
    }
  }, [isRecording, stopRecording]);

  // 组件卸载时确保恢复热键
  useEffect(() => {
    return () => {
      if (isRecording) {
        ipc.hotkeyResume(action).catch(() => {});
      }
    };
  }, [isRecording, action]);

  // 显示文本
  const displayText = isRecording
    ? draft
      ? draft
      : '按下新热键...'
    : value || placeholder;

  return (
    <div className="flex flex-col items-end gap-1">
      <button
        ref={buttonRef}
        type="button"
        onClick={startRecording}
        onKeyDown={handleKeyDown}
        onBlur={handleBlur}
        className={`rounded-lg border px-3 py-1.5 text-sm transition-colors focus:outline-none ${
          isRecording
            ? 'border-[var(--accent)] bg-[var(--accent)]/10 text-[var(--accent)]'
            : 'border-[var(--border)] bg-[var(--surface)] text-[var(--text)] hover:border-[var(--accent)]'
        }`}
        style={{ minWidth: '120px' }}
      >
        {displayText}
      </button>
      {error && <div className="text-xs text-red-500">{error}</div>}
    </div>
  );
}
