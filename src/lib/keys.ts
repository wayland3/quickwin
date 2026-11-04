/**
 * 键盘事件到 Tauri accelerator 格式的转换
 *
 * Tauri 使用 muda crate 的 accelerator 解析器，格式为：
 * - 修饰键：Ctrl, Alt, Shift, Super
 * - 主键：字母、数字、功能键、特殊键名
 * - 示例：Ctrl+Alt+K, Alt+Space, Shift+F1
 */

/**
 * 修饰键的 `event.key` 取值集合
 *
 * 注意：Win/Super 键的 `event.key` 在不同平台上不一致：
 * - macOS / 多数 Chromium：`Meta`
 * - Linux X11（部分桌面环境）：`Super`
 * - 某些键盘/输入法：`OS`、`Hyper`
 *
 * 只认 `Meta` 会把 Linux 上的 Win 键当成「主键」，生成 `Ctrl+Super`
 * 这种缺主键的残缺组合，后端解析失败（InvalidFormat）。
 */
const MODIFIER_KEYS = new Set([
  'Control',
  'Alt',
  'Shift',
  'Meta',
  'Super',
  'OS',
  'Hyper',
  'Win',
]);

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
  // Win/Super：优先用 metaKey；Linux X11 上个别环境不置 metaKey，
  // 此时按 event.key 是否属于 Super 系按键兜底
  if (event.metaKey || keyIsSuper(event.key)) modifiers.push('Super');
  // 去重（metaKey 与 key 兜底可能同时命中）
  const uniqueModifiers = Array.from(new Set(modifiers));

  // 获取主键（排除修饰键本身）
  const key = event.key;
  if (MODIFIER_KEYS.has(key)) {
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
  if (uniqueModifiers.length === 0) {
    return null;
  }

  // 组合：修饰键 + 主键
  return [...uniqueModifiers, mainKey].join('+');
}

/**
 * 判断按键名是否属于 Win/Super 系
 *
 * 不同平台/桌面环境对 Win 键的 `event.key` 命名不一致，
 * 统一映射到 Tauri 的 `Super`。
 */
function keyIsSuper(key: string): boolean {
  return key === 'Super' || key === 'OS' || key === 'Win' || key === 'Hyper';
}

/**
 * 判断当前键盘事件中是否有修饰键处于按下状态
 *
 * 用于热键录制：按 Win/Super 键会唤起系统开始菜单或桌面 Activities，
 * 导致窗口失焦。若失焦时修饰键仍按住，说明用户正在录入含该修饰键的组合，
 * 此时不应退出录制态。
 *
 * @param event 键盘事件
 * @returns 是否有修饰键按住
 */
export function isModifierHeld(event: KeyboardEvent): boolean {
  return (
    event.ctrlKey ||
    event.altKey ||
    event.shiftKey ||
    event.metaKey ||
    keyIsSuper(event.key) ||
    MODIFIER_KEYS.has(event.key)
  );
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
