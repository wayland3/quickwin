import { useCallback, useEffect, useRef, useState } from 'react';
import { ipc } from '../lib/ipc';
import type { LauncherItem } from '../lib/launcher';

const DEBOUNCE_MS = 80;
const HINT_MS = 3000;
const EMPTY_QUERY_PLACEHOLDER = '搜索应用';

/** 启动器查询词跨隐藏保留（SPEC §4：启动器保留上一次查询） */
let persistedQuery = '';

function errorMessage(err: unknown): string {
  if (err && typeof err === 'object' && 'message' in err) {
    const message = (err as { message: unknown }).message;
    if (typeof message === 'string' && message) return message;
  }
  return '搜索失败，请稍后重试';
}

function groupLabel(kind: LauncherItem['kind']): string {
  switch (kind) {
    case 'web':
      return '网页';
    case 'command':
      return '命令';
    case 'file':
      return '文件';
    default:
      return '应用';
  }
}

export function LauncherPanel() {
  const [query, setQuery] = useState(persistedQuery);
  const [items, setItems] = useState<LauncherItem[]>([]);
  const [loading, setLoading] = useState(true);
  const [error, setError] = useState('');
  const [hint, setHint] = useState('');
  const [selected, setSelected] = useState(0);

  const inputRef = useRef<HTMLInputElement>(null);
  const genRef = useRef(0);
  const hintTimerRef = useRef<number | undefined>(undefined);
  const selectedRef = useRef(0);

  const showHint = useCallback((message: string) => {
    setHint(message);
    if (hintTimerRef.current) window.clearTimeout(hintTimerRef.current);
    hintTimerRef.current = window.setTimeout(() => setHint(''), HINT_MS);
  }, []);

  // 打开面板即聚焦输入框
  useEffect(() => {
    inputRef.current?.focus();
  }, []);

  useEffect(() => {
    return () => {
      if (hintTimerRef.current) window.clearTimeout(hintTimerRef.current);
    };
  }, []);

  useEffect(() => {
    persistedQuery = query;
    const generation = ++genRef.current;
    const timer = window.setTimeout(() => {
      setLoading(true);
      ipc
        .launcherQuery(query, 12)
        .then((result) => {
          if (generation !== genRef.current) return;
          setItems(result.items);
          setSelected(0);
          selectedRef.current = 0;
          setError('');
        })
        .catch((err) => {
          if (generation !== genRef.current) return;
          setItems([]);
          setError(errorMessage(err));
        })
        .finally(() => {
          if (generation === genRef.current) setLoading(false);
        });
    }, DEBOUNCE_MS);

    return () => window.clearTimeout(timer);
  }, [query]);

  const move = (delta: number) => {
    if (items.length === 0) return;
    const next = Math.min(Math.max(selectedRef.current + delta, 0), items.length - 1);
    selectedRef.current = next;
    setSelected(next);
  };

  const handleKeyDown = (event: React.KeyboardEvent<HTMLElement>) => {
    switch (event.key) {
      case 'ArrowDown':
        event.preventDefault();
        move(1);
        break;
      case 'ArrowUp':
        event.preventDefault();
        move(-1);
        break;
      case 'Enter': {
        event.preventDefault();
        const current = items[selectedRef.current];
        if (!current) break;
        showHint(current.kind === 'web' ? '网页搜索将在 T-305 接入' : '打开功能将在 T-304 接入');
        break;
      }
      default:
        break;
    }
  };

  return (
    <section className="qw-panel" onKeyDown={handleKeyDown} aria-label="启动器">
      <header className="qw-search">
        <span className="qw-search-icon" aria-hidden="true">
          🔍
        </span>
        <input
          ref={inputRef}
          className="qw-search-input"
          value={query}
          placeholder={EMPTY_QUERY_PLACEHOLDER}
          onChange={(event) => setQuery(event.target.value)}
          aria-label="搜索应用"
        />
        <span className="qw-count">{loading ? '搜索中…' : `${items.length} 条`}</span>
      </header>

      <div className="qw-list" role="listbox" aria-label="搜索结果">
        {items.length === 0 && !loading ? (
          <div className="qw-empty">
            <div className="qw-empty-icon" aria-hidden="true">
              🔍
            </div>
            <div className="qw-empty-title">没有找到应用</div>
            <div className="qw-empty-hint">换个别名，或稍等索引刷新</div>
          </div>
        ) : (
          items.map((item, index) => {
            const showGroup = index === 0 || items[index - 1]?.kind !== item.kind;
            return (
              <div key={item.key}>
                {showGroup && (
                  <div className="px-3 pt-2 pb-1 text-[11px] text-slate-500" role="presentation">
                    {groupLabel(item.kind)}
                  </div>
                )}
                <button
                  type="button"
                  role="option"
                  aria-selected={index === selected}
                  className="qw-row"
                  onClick={() => {
                    selectedRef.current = index;
                    setSelected(index);
                  }}
                  onDoubleClick={() => showHint('打开功能将在 T-304 接入')}
                >
                  <span className="qw-row-icon" aria-hidden="true">
                    {item.kind === 'web' ? '🌐' : '▣'}
                  </span>
                  <span className="qw-row-preview">{item.title}</span>
                  {item.subtitle && <span className="qw-row-meta">{item.subtitle}</span>}
                </button>
              </div>
            );
          })
        )}
      </div>

      <footer className="qw-footer" aria-live="polite">
        <span className="qw-footer-text">
          {error || hint || '↑↓ 选择 · Enter 打开 · Esc 关闭'}
        </span>
      </footer>
    </section>
  );
}
