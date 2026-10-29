import { useCallback, useEffect, useRef, useState } from 'react';
import { listen } from '@tauri-apps/api/event';
import { ipc } from '../lib/ipc';
import {
  cycleFilter,
  dividerBeforeIndex,
  FILTERS,
  formatCount,
  mapClipboardKey,
  moveSelection,
  type ClipFilter,
  type ClipboardDetail,
  type ClipboardItem,
} from '../lib/clipboard';
import { FilterChips } from '../components/FilterChips';
import { ItemRow } from '../components/ItemRow';
import { PreviewPane } from '../components/PreviewPane';
import { KeyHint } from '../components/KeyHint';
import { EmptyState } from '../components/EmptyState';
import { Skeleton } from '../components/Skeleton';

const PAGE_SIZE = 50;
const DEBOUNCE_MS = 80;
const HINT_MS = 3000;
const PAGE_JUMP = 10;

function errorMessage(err: unknown): string {
  if (err && typeof err === 'object' && 'message' in err) {
    const message = (err as { message: unknown }).message;
    if (typeof message === 'string' && message) return message;
  }
  return '操作失败，请稍后重试';
}

export function ClipboardPanel() {
  const [query, setQuery] = useState('');
  const [filter, setFilter] = useState<ClipFilter>('all');
  const [items, setItems] = useState<ClipboardItem[]>([]);
  const [total, setTotal] = useState(0);
  const [loading, setLoading] = useState(true);
  const [loadingMore, setLoadingMore] = useState(false);
  const [error, setError] = useState('');
  const [hint, setHint] = useState('');
  const [selectedId, setSelectedId] = useState<number | null>(null);
  const [detail, setDetail] = useState<ClipboardDetail | null>(null);
  const [detailLoading, setDetailLoading] = useState(false);

  const inputRef = useRef<HTMLInputElement>(null);
  const stateRef = useRef({ query, filter });
  const itemsRef = useRef<ClipboardItem[]>([]);
  const totalRef = useRef(0);
  const genRef = useRef(0);
  const detailReqRef = useRef(0);
  const composingRef = useRef(false);
  const hintTimerRef = useRef<number | undefined>(undefined);
  const moreLoadingRef = useRef(false);
  const selectedIdRef = useRef<number | null>(null);
  const pendingNavRef = useRef<number | null>(null);
  const inputAtRef = useRef(0);

  stateRef.current = { query, filter };
  itemsRef.current = items;
  totalRef.current = total;
  selectedIdRef.current = selectedId;

  /** 临时提示（3s 后恢复底栏默认文案） */
  const showHint = useCallback((message: string) => {
    setHint(message);
    if (hintTimerRef.current !== undefined) {
      window.clearTimeout(hintTimerRef.current);
    }
    hintTimerRef.current = window.setTimeout(() => setHint(''), HINT_MS);
  }, []);

  /**
   * 拉取并替换列表（首屏 / 搜索 / 刷新共用）
   *
   * - 按页拉到 `targetCount`，覆盖用户已加载的范围（刷新不打回第一页）
   * - `preferredId` 优先保持选中；不存在则回落到第一个
   */
  const fetchAll = useCallback(
    async (targetCount = PAGE_SIZE, preferredId: number | null = null) => {
      const gen = ++genRef.current;
      const { query: currentQuery, filter: currentFilter } = stateRef.current;
      const pages = Math.max(1, Math.ceil(targetCount / PAGE_SIZE));
      const startedAt = performance.now();

      setLoading(true);
      setError('');
      moreLoadingRef.current = true;

      let collected: ClipboardItem[] = [];
      let latestTotal = 0;

      try {
        for (let page = 0; page < pages; page += 1) {
          const offset = page * PAGE_SIZE;
          const result = await ipc.clipboardList({
            query: currentQuery,
            filter: currentFilter,
            limit: PAGE_SIZE,
            offset,
          });
          // 条件已变化：整条丢弃（generation 严格匹配）
          if (gen !== genRef.current) return;

          collected = collected.concat(result.items);
          latestTotal = result.total;
          if (result.items.length < PAGE_SIZE || collected.length >= result.total) break;
        }

        // 多页拼接按 id 去重（与 loadMore 保持一致）
        const seen = new Set<number>();
        collected = collected.filter((item) => {
          if (seen.has(item.id)) return false;
          seen.add(item.id);
          return true;
        });

        itemsRef.current = collected;
        totalRef.current = latestTotal;
        setItems(collected);
        setTotal(latestTotal);

        const keep = preferredId ?? selectedIdRef.current;
        const hasKeep = keep !== null && collected.some((item) => item.id === keep);
        if (!hasKeep && keep !== null) {
          // 条目已消失：明确告知用户，而不是静默回落到首项
          showHint('列表已更新');
        }
        setSelectedId(hasKeep ? keep : (collected[0]?.id ?? null));

        // 性能打点（仅开发模式，只输出耗时，不含任何剪贴板内容）
        if (import.meta.env.DEV) {
          window.requestAnimationFrame(() => {
            const commitAt = performance.now();
            const a = commitAt - startedAt;
            const b = inputAtRef.current > 0 ? commitAt - inputAtRef.current : a;
            console.debug(
              `[perf] clipboard list A=${a.toFixed(1)}ms B=${b.toFixed(1)}ms`,
            );
          });
        }
      } catch (err) {
        if (gen !== genRef.current) return;
        setError(errorMessage(err));
      } finally {
        if (gen === genRef.current) {
          setLoading(false);
          moreLoadingRef.current = false;
        }
      }
    },
    [showHint],
  );

  /** 滚到底部时追加下一页 */
  const loadMore = useCallback(async () => {
    if (moreLoadingRef.current) return;
    const loaded = itemsRef.current.length;
    if (loaded >= totalRef.current) return;

    const gen = genRef.current;
    const { query: currentQuery, filter: currentFilter } = stateRef.current;

    moreLoadingRef.current = true;
    setLoadingMore(true);

    try {
      const result = await ipc.clipboardList({
        query: currentQuery,
        filter: currentFilter,
        limit: PAGE_SIZE,
        offset: loaded,
      });
      // 只接受同一 generation 的响应，且按 id 去重（防御并发写导致的 offset 漂移）
      if (gen !== genRef.current) return;

      const seen = new Set(itemsRef.current.map((item) => item.id));
      const merged = itemsRef.current.concat(
        result.items.filter((item) => !seen.has(item.id)),
      );
      itemsRef.current = merged;
      totalRef.current = result.total;
      setItems(merged);
      setTotal(result.total);

      // 跨页导航意图：加载完成后只执行最后一次按键的位移
      const pending = pendingNavRef.current;
      if (pending !== null) {
        pendingNavRef.current = null;
        const index = merged.findIndex((item) => item.id === selectedIdRef.current);
        const target = merged[moveSelection(index, pending, merged.length)];
        if (target) setSelectedId(target.id);
      }
    } catch (err) {
      if (gen === genRef.current) setError(errorMessage(err));
    } finally {
      if (gen === genRef.current) {
        setLoadingMore(false);
        moreLoadingRef.current = false;
      }
    }
  }, []);

  // 搜索 / 筛选变化：防抖 80ms 后重置到首屏
  useEffect(() => {
    const timer = window.setTimeout(() => {
      void fetchAll(PAGE_SIZE);
    }, DEBOUNCE_MS);
    return () => window.clearTimeout(timer);
  }, [query, filter, fetchAll]);

  // 选中变化：拉详情（requestId 丢弃过期响应）
  useEffect(() => {
    if (selectedId === null) {
      setDetail(null);
      return;
    }

    const requestId = ++detailReqRef.current;
    setDetailLoading(true);

    ipc
      .clipboardGet(selectedId)
      .then((result) => {
        if (requestId === detailReqRef.current) setDetail(result);
      })
      .catch(() => {
        if (requestId !== detailReqRef.current) return;
        // 条目已不存在（被清理 / 删除）：提示并刷新列表（不新增错误码）
        setDetail(null);
        showHint('条目已不存在，列表已刷新');
        void fetchAll(PAGE_SIZE);
      })
      .finally(() => {
        if (requestId === detailReqRef.current) setDetailLoading(false);
      });
  }, [selectedId, fetchAll, showHint]);

  // 面板打开期间监听入库事件；刷新到已加载的条数，不打回第一页
  useEffect(() => {
    const unlisten = listen('clipboard://changed', () => {
      const loaded = Math.max(PAGE_SIZE, itemsRef.current.length);
      void fetchAll(loaded);
    });
    return () => {
      unlisten.then((off) => off());
    };
  }, [fetchAll]);

  // 焦点双保险：autoFocus + 挂载后再确认一次（防止首字符丢失）
  useEffect(() => {
    const focusInput = () => inputRef.current?.focus();
    focusInput();
    if (document.activeElement !== inputRef.current) {
      const timer = window.setTimeout(focusInput, 0);
      return () => window.clearTimeout(timer);
    }
  }, []);

  useEffect(() => {
    return () => {
      if (hintTimerRef.current !== undefined) {
        window.clearTimeout(hintTimerRef.current);
      }
    };
  }, []);

  // 选中项变化时保证可见（不改变列表滚动锚点）
  useEffect(() => {
    if (selectedId === null) return;
    const node = document.querySelector(`[data-item-id="${selectedId}"]`);
    // jsdom 没有 scrollIntoView，运行时做能力判断
    if (node && typeof node.scrollIntoView === 'function') {
      node.scrollIntoView({ block: 'nearest' });
    }
  }, [selectedId, items]);

  const moveBy = (delta: number) => {
    const list = itemsRef.current;
    const index = list.findIndex((item) => item.id === selectedIdRef.current);
    const target = list[moveSelection(index, delta, list.length)];
    if (target) setSelectedId(target.id);
  };

  const handleKeyDown = (event: React.KeyboardEvent<HTMLElement>) => {
    const action = mapClipboardKey({
      key: event.key,
      ctrlKey: event.ctrlKey,
      shiftKey: event.shiftKey,
      metaKey: event.metaKey,
      altKey: event.altKey,
      isComposing: composingRef.current || event.nativeEvent.isComposing,
    });

    if (action === 'none') return;
    event.preventDefault();

    const currentIndex = items.findIndex((item) => item.id === selectedId);
    const current = currentIndex >= 0 ? items[currentIndex] : null;

    switch (action) {
      case 'up':
        moveBy(-1);
        break;
      case 'down':
        moveBy(1);
        break;
      case 'page-up':
      case 'page-down': {
        const delta = action === 'page-up' ? -PAGE_JUMP : PAGE_JUMP;
        // 跨已加载页边界：先加载，加载完成后只执行最后一次导航意图
        if (currentIndex + delta >= items.length && items.length < total) {
          pendingNavRef.current = delta;
          void loadMore();
        } else {
          moveBy(delta);
        }
        break;
      }
      case 'pin':
        if (!current) break;
        ipc
          .clipboardSetPinned(current.id, !current.pinned)
          .then(() => fetchAll(Math.max(PAGE_SIZE, items.length), current.id))
          .catch((err) => setError(errorMessage(err)));
        break;
      case 'delete':
      case 'hard-delete': {
        if (!current) break;
        // 落点：优先下一项，删除末项时回退上一项（N2）
        const nextId = items[currentIndex + 1]?.id ?? items[currentIndex - 1]?.id ?? null;
        ipc
          .clipboardDelete(current.id, action === 'hard-delete')
          // 保持已加载范围，避免在第 2+ 页删除后被打回首屏（N2）
          .then(() => fetchAll(Math.max(PAGE_SIZE, items.length), nextId))
          .catch((err) => setError(errorMessage(err)));
        break;
      }
      case 'paste':
        if (!current) break;
        ipc.clipboardActivate(current.id, 'paste').catch((err) => {
          setError(errorMessage(err));
        });
        break;
      case 'copy':
        if (!current) break;
        ipc
          .clipboardActivate(current.id, 'copy')
          .then(() => showHint('已复制'))
          .catch((err) => setError(errorMessage(err)));
        break;
      case 'filter-next':
        setFilter((prev) => cycleFilter(prev, 1));
        break;
      case 'filter-prev':
        setFilter((prev) => cycleFilter(prev, -1));
        break;
      case 'escape':
        ipc.overlayHide().catch(() => undefined);
        break;
      default:
        break;
    }
  };

  const handleScroll = (event: React.UIEvent<HTMLDivElement>) => {
    const element = event.currentTarget;
    if (element.scrollHeight - element.scrollTop - element.clientHeight <= 120) {
      void loadMore();
    }
  };

  const filterLabel = FILTERS.find((entry) => entry.id === filter)?.label ?? '全部';
  const dividerIndex = dividerBeforeIndex(items);

  return (
    <section className="qw-panel" onKeyDown={handleKeyDown} aria-label="剪贴板历史">
      <header className="qw-search">
        <span className="qw-search-icon" aria-hidden="true">
          🔍
        </span>
        <input
          ref={inputRef}
          className="qw-search-input"
          value={query}
          onChange={(event) => {
            // 性能打点 B 口径的起点（最后一次按键）
            inputAtRef.current = performance.now();
            setQuery(event.target.value);
          }}
          onCompositionStart={() => {
            composingRef.current = true;
          }}
          onCompositionEnd={() => {
            composingRef.current = false;
          }}
          placeholder="搜索剪贴板"
          aria-label="搜索剪贴板"
          autoFocus
          spellCheck={false}
        />
        <FilterChips value={filter} onChange={setFilter} />
        <span className="qw-count">{formatCount(total)} 条</span>
      </header>

      <div className="qw-main">
        <div
          className="qw-list"
          role="listbox"
          aria-label="剪贴板历史条目"
          onScroll={handleScroll}
        >
          {loading && items.length === 0 ? (
            <Skeleton />
          ) : items.length === 0 ? (
            <EmptyState query={query} filterLabel={filterLabel} />
          ) : (
            items.map((item, index) => (
              <div key={item.id}>
                {index === dividerIndex && (
                  <div className="qw-divider" role="separator" />
                )}
                <ItemRow
                  item={item}
                  selected={item.id === selectedId}
                  onSelect={setSelectedId}
                />
              </div>
            ))
          )}

          {loadingMore && (
            <div className="qw-skeleton-row qw-skeleton-row-single" aria-hidden="true" />
          )}
        </div>

        <PreviewPane detail={detail} loading={detailLoading} />
      </div>

      <KeyHint hint={hint} total={total} error={error} />
    </section>
  );
}

