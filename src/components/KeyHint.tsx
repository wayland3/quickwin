import { formatCount } from '../lib/clipboard';

interface KeyHintProps {
  /** 临时提示（开发中 / 操作反馈）；为空时显示默认键位 */
  hint: string;
  total: number;
  /** 错误原因；有值时提示条变 warn 色 */
  error: string;
}

/** 底部提示条（28px，UI-SPEC §4.1 / §6） */
export function KeyHint({ hint, total, error }: KeyHintProps) {
  const text =
    hint ||
    '↑↓ 选择 · Enter 粘贴 · Ctrl+Enter 仅复制 · Ctrl+P 置顶 · Del 删除 · Tab 筛选 · Esc 关闭';

  return (
    <footer
      className={`qw-footer${error ? ' qw-footer-error' : ''}`}
      aria-live="polite"
    >
      <span className="qw-footer-text">{error || text}</span>
      <span className="qw-footer-count">{formatCount(total)} 条</span>
    </footer>
  );
}
