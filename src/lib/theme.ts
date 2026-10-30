/**
 * 主题应用逻辑
 *
 * 通过修改 <html data-theme> 属性切换主题：
 * - 'system'：移除属性，跟随系统 prefers-color-scheme
 * - 'light' / 'dark'：设置属性，覆盖系统偏好
 */
export type Theme = 'system' | 'light' | 'dark';

/**
 * 应用主题到 DOM
 *
 * @param theme - 主题名称：'system' | 'light' | 'dark'
 */
export function applyTheme(theme: Theme): void {
  if (theme === 'system') {
    document.documentElement.removeAttribute('data-theme');
  } else if (theme === 'light' || theme === 'dark') {
    document.documentElement.setAttribute('data-theme', theme);
  } else {
    // 非法值兜底：视为 system
    console.warn(`Unknown theme: ${theme}, falling back to system`);
    document.documentElement.removeAttribute('data-theme');
  }
}
