import { useCallback, useEffect, useRef, useState } from 'react';
import { eventToAccelerator, isSuperKeyName } from '../lib/keys';
import { ipc } from '../lib/ipc';

/** 非 Win 的修饰键按键名（按住时只等主键，不做处理） */
const MODIFIER_KEY_NAMES = new Set(['Control', 'Alt', 'Shift']);

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
 *
 * # Linux 上 Win 键的特殊处理
 *
 * 按 Win/Super 键会唤起桌面的 Activities / 应用菜单，窗口随之失焦。
 * 窗口一旦失焦，绑定在按钮上的 `onKeyDown` 就收不到后续的 Space 等主键，
 * 只能拿到「Ctrl+Super」这种缺主键的残缺组合（后端解析失败）。
 *
 * 因此录制态改用 **window 捕获阶段** 监听键盘事件：只要 WebView 还活着，
 * 即便窗口失焦也能收到按键。同时失焦不再立即退出录制，而是延迟兜底。
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
  const blurTimeoutRef = useRef<number | null>(null);
  const recordingRef = useRef(false);
  /** 最近一次按键后是否已有主键（用于失焦兜底判断） */
  const gotMainKeyRef = useRef(false);
  /** Win/Super 键是否处于按住状态（Linux 上主键事件的 metaKey 可能不置位） */
  const superHeldRef = useRef(false);

  useEffect(() => {
    recordingRef.current = isRecording;
  }, [isRecording]);

  // 进入录制态
  const startRecording = useCallback(async () => {
    setIsRecording(true);
    setDraft(null);
    setError(null);
    gotMainKeyRef.current = false;
    superHeldRef.current = false;

    // 暂停全局热键，避免录制时误触发
    try {
      await ipc.hotkeySuspend(action);
    } catch {
      // 暂停失败不阻断录制，只是可能误触发
    }

    buttonRef.current?.focus();
  }, [action]);

  // 退出录制态
  const stopRecording = useCallback(async () => {
    setIsRecording(false);
    setDraft(null);
    gotMainKeyRef.current = false;
    superHeldRef.current = false;

    if (blurTimeoutRef.current !== null) {
      window.clearTimeout(blurTimeoutRef.current);
      blurTimeoutRef.current = null;
    }

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

  // 全局键盘监听（捕获阶段）
  //
  // 用 window + capture 而不是按钮的 onKeyDown：窗口失焦后按钮收不到事件。
  useEffect(() => {
    if (!isRecording) return;

    const onKeyDown = (event: KeyboardEvent) => {
      event.preventDefault();
      event.stopPropagation();

      // Win/Super 键本身：只记住「按住」状态，不算主键
      //
      // Linux 上按 Win 会立刻让窗口失焦，且后续主键事件的 metaKey 可能为 false。
      // 记住这个状态，构造组合时补上 Super。
      if (isSuperKeyName(event.key)) {
        superHeldRef.current = true;
        return;
      }

      if (MODIFIER_KEY_NAMES.has(event.key)) {
        // 其他修饰键只等主键，不做任何事
        return;
      }

      if (event.key === 'Escape') {
        void stopRecording();
        return;
      }

      if (event.key === 'Backspace') {
        void clearHotkey();
        return;
      }

      const accelerator = eventToAccelerator(event, {
        superHeld: superHeldRef.current,
      });
      if (accelerator) {
        gotMainKeyRef.current = true;
        setDraft(accelerator);
        void saveHotkey(accelerator);
      }
    };

    // 修饰键全部松开 → 退出录制态
    //
    // 保存成功时 saveHotkey 已停止录制；这里兜底「保存失败」与
    // 「只按了修饰键就松手」两种情况，避免卡在录制态。错误提示保留。
    const onKeyUp = (event: KeyboardEvent) => {
      if (isSuperKeyName(event.key)) {
        superHeldRef.current = false;
      }
      const modifierStillHeld =
        event.ctrlKey ||
        event.altKey ||
        event.shiftKey ||
        event.metaKey ||
        superHeldRef.current;
      if (modifierStillHeld) return;
      void stopRecording();
    };

    window.addEventListener('keydown', onKeyDown, true);
    window.addEventListener('keyup', onKeyUp, true);
    return () => {
      window.removeEventListener('keydown', onKeyDown, true);
      window.removeEventListener('keyup', onKeyUp, true);
    };
  }, [isRecording, stopRecording, clearHotkey, saveHotkey]);

  // 失焦兜底：延迟退出，给用户按主键的时间
  //
  // Linux 上按 Win 必然失焦；800ms 内若没有拿到主键才退出。
  const handleBlur = useCallback(() => {
    if (!isRecording) return;

    if (blurTimeoutRef.current !== null) {
      window.clearTimeout(blurTimeoutRef.current);
    }

    blurTimeoutRef.current = window.setTimeout(() => {
      blurTimeoutRef.current = null;
      if (!recordingRef.current) return;
      // 已拿到主键、或 Win 键仍按住：不退出，继续等主键
      if (gotMainKeyRef.current || superHeldRef.current || draft) return;
      void stopRecording();
    }, 800);
  }, [isRecording, draft, stopRecording]);

  // 组件卸载时确保恢复热键
  useEffect(() => {
    return () => {
      if (recordingRef.current) {
        ipc.hotkeyResume(action).catch(() => {});
      }
      if (blurTimeoutRef.current !== null) {
        window.clearTimeout(blurTimeoutRef.current);
      }
    };
  }, [action]);

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
        onClick={() => void startRecording()}
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
