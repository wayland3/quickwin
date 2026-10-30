import { describe, it, expect, beforeEach, afterEach } from 'vitest';
import { applyTheme, type Theme } from './theme';

describe('主题系统', () => {
  beforeEach(() => {
    // 每个测试前清理 data-theme 属性
    document.documentElement.removeAttribute('data-theme');
  });

  afterEach(() => {
    // 每个测试后清理
    document.documentElement.removeAttribute('data-theme');
  });

  describe('applyTheme 函数', () => {
    it('system 主题移除 data-theme 属性', () => {
      // 先设置一个主题
      document.documentElement.setAttribute('data-theme', 'dark');
      
      applyTheme('system');
      
      expect(document.documentElement.getAttribute('data-theme')).toBeNull();
    });

    it('light 主题设置 data-theme 为 light', () => {
      applyTheme('light');
      
      expect(document.documentElement.getAttribute('data-theme')).toBe('light');
    });

    it('dark 主题设置 data-theme 为 dark', () => {
      applyTheme('dark');
      
      expect(document.documentElement.getAttribute('data-theme')).toBe('dark');
    });

    it('非法主题值兜底为 system（移除属性）', () => {
      // 先设置一个主题
      document.documentElement.setAttribute('data-theme', 'dark');
      
      // 传入非法值（类型断言绕过 TS 检查）
      applyTheme('invalid' as unknown as Theme);
      
      expect(document.documentElement.getAttribute('data-theme')).toBeNull();
    });

    it('从 light 切换到 dark 正确更新属性', () => {
      applyTheme('light');
      expect(document.documentElement.getAttribute('data-theme')).toBe('light');
      
      applyTheme('dark');
      expect(document.documentElement.getAttribute('data-theme')).toBe('dark');
    });

    it('从 dark 切换到 system 正确移除属性', () => {
      applyTheme('dark');
      expect(document.documentElement.getAttribute('data-theme')).toBe('dark');
      
      applyTheme('system');
      expect(document.documentElement.getAttribute('data-theme')).toBeNull();
    });
  });
});
