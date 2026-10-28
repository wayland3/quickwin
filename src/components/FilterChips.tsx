import { FILTERS, type ClipFilter } from '../lib/clipboard';

interface FilterChipsProps {
  value: ClipFilter;
  onChange: (filter: ClipFilter) => void;
}

/** 过滤 chip：鼠标点击可用，键盘用 Tab / Shift+Tab 循环 */
export function FilterChips({ value, onChange }: FilterChipsProps) {
  return (
    <div className="qw-chips" role="tablist" aria-label="筛选类型">
      {FILTERS.map((filter) => (
        <button
          key={filter.id}
          type="button"
          role="tab"
          aria-selected={value === filter.id}
          tabIndex={-1}
          className="qw-chip"
          onClick={() => onChange(filter.id)}
        >
          {filter.label}
        </button>
      ))}
    </div>
  );
}
