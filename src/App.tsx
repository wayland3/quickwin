import { useEffect, useState } from 'react';
import { ipc } from './lib/ipc';
import { listen } from '@tauri-apps/api/event';
import { ClipboardPanel } from './windows/ClipboardPanel';
import { LauncherPanel } from './windows/LauncherPanel';

function App() {
  const [pingResult, setPingResult] = useState<string>('');
  const [error, setError] = useState<string>('');
  const [mode, setMode] = useState<string>('');
  const [toast, setToast] = useState<{ message: string; level: string } | null>(null);

  useEffect(() => {
    if (!toast) return;
    const timer = window.setTimeout(() => setToast(null), 4000);
    return () => window.clearTimeout(timer);
  }, [toast]);

  useEffect(() => {
    ipc.appPing()
      .then((result) => setPingResult(result))
      .catch((err) => setError(String(err)));

    // 监听 overlay://show 事件（来自托盘菜单）：同时重查失败热键，保证托盘直达也有提示
    const unlistenShow = listen<string>('overlay://show', (event) => {
      setMode(event.payload);
      showFailedHotkeys();
    });

    // 监听 overlay://shown 事件（浮层显示后）：窗口此刻对用户可见，重新检查失败热键
    const unlistenShown = listen<string>('overlay://shown', (event) => {
      setMode(event.payload);
      showFailedHotkeys();
    });

    // 监听 overlay://hidden 事件（浮层隐藏后）
    const unlistenHidden = listen('overlay://hidden', () => {
      setMode('');
    });

    // 监听热键按下事件
    const unlistenHotkey = listen<string>('hotkey://pressed', (event) => {
      const action = event.payload;
      if (action === 'launcher') {
        ipc.overlayShow('launcher').catch(console.error);
      } else if (action === 'clipboard') {
        ipc.overlayShow('clipboard').catch(console.error);
      } else if (action === 'actions') {
        ipc.overlayShow('actions').catch(console.error);
      }
    });

    // setup 阶段的注册失败事件早于本监听器建立，无法接收；改为挂载后主动拉取状态
    const showFailedHotkeys = () => {
      ipc
        .hotkeyStatus()
        .then((status) => {
          if (status.failed.length > 0) {
            const keys = status.failed.map((item) => item.accelerator).join('、');
            setToast({
              message: `热键 ${keys} 已被其他程序占用，可在设置中改绑`,
              level: 'info',
            });
          }
        })
        .catch(() => undefined);
    };
    showFailedHotkeys();

    // 运行时注册失败（如改绑）通过事件即时提示
    const unlistenHotkeyFailed = listen<{ accelerator?: string }>(
      'hotkey://failed',
      (event) => {
        const accelerator = event.payload?.accelerator;
        if (!accelerator) return;
        setToast({
          message: `热键 ${accelerator} 已被其他程序占用，可在设置中改绑`,
          level: 'info',
        });
      },
    );

    // Esc 键隐藏浮层
    const handleKeyDown = (e: KeyboardEvent) => {
      if (e.key === 'Escape') {
        ipc.overlayHide().catch(console.error);
      }
    };
    window.addEventListener('keydown', handleKeyDown);

    // 剪贴板入库失败提示
    const unlistenInsertError = listen<{ code: string; message: string }>(
      'clipboard://insert-error',
      (event) => {
        const message = event.payload?.message || '剪贴板记录失败';
        setToast({ message, level: 'error' });
      },
    );

    // 后端降级提示（粘贴 / 仅复制）
    const unlistenToast = listen<{ level?: string; message?: string }>(
      'app://toast',
      (event) => {
        const message = event.payload?.message;
        if (!message) return;
        setToast({ message, level: event.payload?.level ?? 'info' });
      },
    );

    // 托盘暂停记录事件：转到设置服务（30 分钟=Some(1800)，恢复=Some(0)，直到重启=null）
    const unlistenClipboardPause = listen<number | null>(
      'clipboard://pause',
      (event) => {
        const duration = event.payload;
        ipc
          .settingsSetPaused(duration)
          .then(() => {
            const message =
              duration === null
                ? '已暂停记录（直到重启）'
                : duration === 0
                  ? '已恢复记录'
                  : '已暂停记录 30 分钟';
            setToast({ message, level: 'info' });
          })
          .catch(() => undefined);
      },
    );

    // 设置变更：热键占用等情况通过统一 toast 提示
    const unlistenSettingsChanged = listen<{
      keys: string[];
      failed: string[];
    }>('settings://changed', (event) => {
      const failed = event.payload?.failed ?? [];
      if (failed.length > 0) {
        setToast({
          message: `热键 ${failed.join('、')} 已被其他程序占用`,
          level: 'info',
        });
      }
    });

    return () => {
      unlistenShow.then(f => f());
      unlistenShown.then(f => f());
      unlistenHidden.then(f => f());
      unlistenHotkey.then(f => f());
      unlistenInsertError.then(f => f());
      unlistenToast.then(f => f());
      unlistenHotkeyFailed.then(f => f());
      unlistenClipboardPause.then(f => f());
      unlistenSettingsChanged.then(f => f());
      window.removeEventListener('keydown', handleKeyDown);
    };
  }, []);

  return (
    <main className="qw-root">
      {toast && (
        <div
          role="alert"
          className={
            toast.level === 'error'
              ? 'fixed bottom-4 left-1/2 -translate-x-1/2 rounded-md bg-red-600/90 px-4 py-2 text-sm text-white shadow-lg'
              : 'fixed bottom-4 left-1/2 -translate-x-1/2 rounded-md bg-slate-700/90 px-4 py-2 text-sm text-white shadow-lg'
          }
        >
          {toast.message}
        </div>
      )}

      {mode === 'clipboard' ? (
        <ClipboardPanel />
      ) : mode === 'launcher' ? (
        <LauncherPanel />
      ) : (
        <div className="qw-placeholder">
          <h1 className="mb-4 text-4xl font-bold">QuickWin</h1>
          <p className="mb-2 text-slate-400">脚手架已就绪，等待业务开发</p>
          {mode && <p className="mt-4 text-sm text-blue-400">当前模式：{mode}</p>}
          {pingResult && (
            <p className="mt-4 text-sm text-green-400">后端连通：{pingResult}</p>
          )}
          {error && <p className="mt-4 text-sm text-red-400">后端未就绪：{error}</p>}
        </div>
      )}
    </main>
  );
}

export default App;
