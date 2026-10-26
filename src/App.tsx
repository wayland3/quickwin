import { useEffect, useState } from 'react';
import { ipc } from './lib/ipc';

function App() {
  const [pingResult, setPingResult] = useState<string>('');
  const [error, setError] = useState<string>('');

  useEffect(() => {
    ipc.appPing()
      .then((result) => setPingResult(result))
      .catch((err) => setError(String(err)));
  }, []);

  return (
    <main className="flex min-h-screen items-center justify-center bg-slate-900 text-slate-100">
      <div className="text-center">
        <h1 className="mb-4 text-4xl font-bold">QuickWin</h1>
        <p className="mb-2 text-slate-400">脚手架已就绪，等待业务开发</p>
        {pingResult && (
          <p className="mt-4 text-sm text-green-400">
            后端连通：{pingResult}
          </p>
        )}
        {error && (
          <p className="mt-4 text-sm text-red-400">
            后端未就绪：{error}
          </p>
        )}
      </div>
    </main>
  );
}

export default App;
