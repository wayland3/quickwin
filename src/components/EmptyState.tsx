interface EmptyStateProps {
  /** 当前搜索词；为空表示空历史 */
  query: string;
  /** 当前筛选标签，用于空态文案 */
  filterLabel: string;
}

/** 空态 / 无搜索结果（UI-SPEC §6） */
export function EmptyState({ query, filterLabel }: EmptyStateProps) {
  if (query.trim()) {
    return (
      <div className="qw-empty" role="status">
        <span className="qw-empty-icon" aria-hidden="true">
          🔍
        </span>
        <p className="qw-empty-title">没有找到 “{query.trim()}”</p>
        <p className="qw-empty-hint">换个关键词，或按 Tab 切换筛选</p>
      </div>
    );
  }

  return (
    <div className="qw-empty" role="status">
      <span className="qw-empty-icon" aria-hidden="true">
        📋
      </span>
      <p className="qw-empty-title">
        {filterLabel === '全部' ? '还没有记录' : `「${filterLabel}」下还没有记录`}
      </p>
      <p className="qw-empty-hint">复制任意内容后会出现在这里</p>
    </div>
  );
}
