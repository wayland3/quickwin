import { useEffect, useState } from 'react';
import { ipc } from './lib/ipc';
import { listen } from '@tauri-apps/api/event';
import { ClipboardPanel } from './windows/ClipboardPanel';

function App() {
  const [pingResult, setPingResult] = useState<string>('');
  const [error, setError] = useState<string>('');
  const [mode, setMode] = useState<string>('');
  const [toast, setToast] = useState<string>('');

  useEffect(() => {
    if (!toast) return;
    const timer = window.setTimeout(() => setToast(''), 4000);
    return () => window.clearTimeout(timer);
  }, [toast]);

  useEffect(() => {
    ipc.appPing()
      .then((result) => setPingResult(result))
      .catch((err) => setError(String(err)));

    // 监听 overlay://show 事件（来自托盘菜单）
    const unlistenShow = listen<string>('overlay://show', (event) => {
      setMode(event.payload);
    });

    // 监听 overlay://shown 事件（浮层显示后）
    const unlistenShown = listen<string>('overlay://shown', (event) => {
      setMode(event.payload);
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
        setToast(message);
      },
    );

    return () => {
      unlistenShow.then(f => f());
      unlistenShown.then(f => f());
      unlistenHidden.then(f => f());
      unlistenHotkey.then(f => f());
      unlistenInsertError.then(f => f());
      window.removeEventListener('keydown', handleKeyDown);
    };
  }, []);

  return (
    <main className="qw-root">
      {toast && (
        <div
          role="alert"
          className="fixed bottom-4 left-1/2 -translate-x-1/2 rounded-md bg-red-600/90 px-4 py-2 text-sm text-white shadow-lg"
        >
          {toast}
        </div>
      )}

      {mode === 'clipboard' ? (
        <ClipboardPanel />
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
