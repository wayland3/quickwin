import {
  formatRelativeTime,
  kindIcon,
  shouldShowCopyBadge,
  type ClipboardItem,
} from '../lib/clipboard';

interface ItemRowProps {
  item: ClipboardItem;
  selected: boolean;
  onSelect: (id: number) => void;
}

/** 列表行（44px 单行，UI-SPEC §4.3） */
export function ItemRow({ item, selected, onSelect }: ItemRowProps) {
  const meta = [item.source_app || '未知来源', formatRelativeTime(item.last_copied_at)];

  return (
    <div
      role="option"
      aria-selected={selected}
      data-item-id={item.id}
      className="qw-row"
      onClick={() => onSelect(item.id)}
    >
      <span className="qw-row-icon" aria-hidden="true">
        {kindIcon(item.kind)}
      </span>

      <span className="qw-row-preview">{item.preview}</span>

      <span className="qw-row-meta">
        {meta.join(' · ')}
        {shouldShowCopyBadge(item.copy_count) && (
          <span className="qw-badge" aria-label={`已复制 ${item.copy_count} 次`}>
            ×{item.copy_count}
          </span>
        )}
        {item.pinned && (
          <span className="qw-badge-pin" aria-label="已置顶">
            📌
          </span>
        )}
      </span>
    </div>
  );
}
