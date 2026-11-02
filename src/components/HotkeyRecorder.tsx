import { useCallback, useEffect, useRef, useState } from 'react';
import { eventToAccelerator, isModifierHeld } from '../lib/keys';
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
  /** 录制期间是否有修饰键按住（用于失焦时判定是否退出录制） */
  const [modifierHeld, setModifierHeld] = useState(false);
  const buttonRef = useRef<HTMLButtonElement>(null);

  // 进入录制态
  const startRecording = useCallback(async () => {
    setIsRecording(true);
    setDraft(null);
    setError(null);
    setModifierHeld(false);

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
    setModifierHeld(false);

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

      // 记录当前按下的修饰键，供失焦判定使用
      setModifierHeld(isModifierHeld(event.nativeEvent));

      // 尝试构建 accelerator
      const accelerator = eventToAccelerator(event.nativeEvent);
      if (accelerator) {
        setDraft(accelerator);
        await saveHotkey(accelerator);
      }
    },
    [isRecording, stopRecording, clearHotkey, saveHotkey],
  );

  // 键盘事件抬起：更新修饰键状态
  //
  // 兜底：修饰键松开时退出录制态。
  // - 只按了 Win 就松手（没录到组合键）→ 退出，避免停在录制态
  // - 录到了但保存失败 → 也退出，让用户看到错误并决定下一步
  //   （错误信息在退出后依然显示，用户可重新点击录制）
  //
  // 用 ref 而不是 draft 判断是否录到过：draft 在保存失败时仍有值，
  // 若以 draft 为准就会永远停在录制态（修饰键已松开，不再有 keyup 触发）。
  const handleKeyUp = useCallback(
    (event: React.KeyboardEvent) => {
      if (!isRecording) return;
      const stillHeld = isModifierHeld(event.nativeEvent);
      setModifierHeld(stillHeld);
      if (!stillHeld) {
        stopRecording();
      }
    },
    [isRecording, stopRecording],
  );

  // 失焦时退出录制态
  //
  // 例外：修饰键仍按住时不退出。按 Win/Super 键会唤起系统开始菜单或桌面
  // Activities，导致窗口失焦；若此时退出录制，用户就再也录不进 `Win+空格`
  // 这类组合。修饰键松开后若仍未录到组合键，由 keyup 兜底退出录制态。
  const handleBlur = useCallback(() => {
    if (!isRecording) return;
    if (modifierHeld) return;
    stopRecording();
  }, [isRecording, modifierHeld, stopRecording]);

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
        onKeyUp={handleKeyUp}
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
