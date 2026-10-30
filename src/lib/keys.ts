/**
 * 键盘事件到 Tauri accelerator 格式的转换
 *
 * Tauri 使用 muda crate 的 accelerator 解析器，格式为：
 * - 修饰键：Ctrl, Alt, Shift, Super
 * - 主键：字母、数字、功能键、特殊键名
 * - 示例：Ctrl+Alt+K, Alt+Space, Shift+F1
 */

/** 修饰键映射：DOM event 属性 → Tauri 修饰键名 */
const MODIFIER_MAP: Record<string, string> = {
  Control: 'Ctrl',
  Alt: 'Alt',
  Shift: 'Shift',
  Meta: 'Super',
};

/** 特殊键映射：DOM event.key → Tauri 键名 */
const KEY_MAP: Record<string, string> = {
  ' ': 'Space',
  ArrowUp: 'Up',
  ArrowDown: 'Down',
  ArrowLeft: 'Left',
  ArrowRight: 'Right',
  Escape: 'Escape',
  Enter: 'Return',
  Backspace: 'Backspace',
  Delete: 'Delete',
  Tab: 'Tab',
  Home: 'Home',
  End: 'End',
  PageUp: 'PageUp',
  PageDown: 'PageDown',
  Insert: 'Insert',
};

/**
 * 从键盘事件构建 Tauri accelerator 字符串
 *
 * @param event 键盘事件
 * @returns accelerator 字符串，如果无效则返回 null
 */
export function eventToAccelerator(event: KeyboardEvent): string | null {
  const modifiers: string[] = [];
  let mainKey = '';

  // 收集修饰键
  if (event.ctrlKey) modifiers.push('Ctrl');
  if (event.altKey) modifiers.push('Alt');
  if (event.shiftKey) modifiers.push('Shift');
  if (event.metaKey) modifiers.push('Super');

  // 获取主键（排除修饰键本身）
  const key = event.key;
  if (key in MODIFIER_MAP) {
    // 按下的只是修饰键，无效
    return null;
  }

  // 映射特殊键
  if (key in KEY_MAP) {
    mainKey = KEY_MAP[key]!;
  } else if (key.length === 1) {
    // 单字符键（字母、数字、符号）
    mainKey = key.toUpperCase();
  } else if (key.startsWith('F') && key.length <= 3) {
    // 功能键 F1-F24
    mainKey = key;
  } else {
    // 其他键，尝试直接使用
    mainKey = key;
  }

  // 至少需要一个修饰键
  if (modifiers.length === 0) {
    return null;
  }

  // 组合：修饰键 + 主键
  return [...modifiers, mainKey].join('+');
}

/**
 * 格式化 accelerator 字符串用于显示
 *
 * @param accelerator Tauri accelerator 字符串
 * @returns 用户友好的显示格式
 */
export function formatAccelerator(accelerator: string): string {
  return accelerator
    .replace('Ctrl', 'Ctrl')
    .replace('Alt', 'Alt')
    .replace('Shift', 'Shift')
    .replace('Super', 'Win')
    .replace('Space', '空格')
    .replace('Return', 'Enter')
    .replace('Escape', 'Esc');
}

/**
 * 验证 accelerator 是否有效
 *
 * @param accelerator 要验证的字符串
 * @returns 是否有效
 */
export function isValidAccelerator(accelerator: string): boolean {
  if (!accelerator || accelerator.trim() === '') {
    return false;
  }

  const parts = accelerator.split('+');
  if (parts.length < 2) {
    return false;
  }

  // 至少需要一个修饰键
  const validModifiers = ['Ctrl', 'Alt', 'Shift', 'Super'];
  const hasModifier = parts.slice(0, -1).some((p) => validModifiers.includes(p));

  return hasModifier;
}
