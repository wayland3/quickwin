import { describe, expect, it } from 'vitest';
import { eventToAccelerator, formatAccelerator, isValidAccelerator } from './keys';

describe('keys', () => {
  describe('eventToAccelerator', () => {
    function makeKeyEvent(
      key: string,
      options: { ctrl?: boolean; alt?: boolean; shift?: boolean; meta?: boolean } = {},
    ): KeyboardEvent {
      return new KeyboardEvent('keydown', {
        key,
        ctrlKey: options.ctrl ?? false,
        altKey: options.alt ?? false,
        shiftKey: options.shift ?? false,
        metaKey: options.meta ?? false,
      });
    }

    it('字母键加 Ctrl', () => {
      const event = makeKeyEvent('k', { ctrl: true });
      expect(eventToAccelerator(event)).toBe('Ctrl+K');
    });

    it('字母键加 Alt', () => {
      const event = makeKeyEvent('v', { alt: true });
      expect(eventToAccelerator(event)).toBe('Alt+V');
    });

    it('多修饰键组合', () => {
      const event = makeKeyEvent('k', { ctrl: true, alt: true });
      expect(eventToAccelerator(event)).toBe('Ctrl+Alt+K');
    });

    it('空格键', () => {
      const event = makeKeyEvent(' ', { alt: true });
      expect(eventToAccelerator(event)).toBe('Alt+Space');
    });

    it('功能键', () => {
      const event = makeKeyEvent('F1', { shift: true });
      expect(eventToAccelerator(event)).toBe('Shift+F1');
    });

    it('方向键', () => {
      const event = makeKeyEvent('ArrowUp', { ctrl: true });
      expect(eventToAccelerator(event)).toBe('Ctrl+Up');
    });

    it('Meta 键映射为 Super', () => {
      const event = makeKeyEvent('k', { meta: true });
      expect(eventToAccelerator(event)).toBe('Super+K');
    });

    it('Linux X11 上 Win 键名 Super/OS/Win 不应被当成主键', () => {
      // 回归：此前只认 Meta，导致生成 'Ctrl+Super' 这种缺主键的残缺组合
      expect(eventToAccelerator(makeKeyEvent('Super', { ctrl: true }))).toBeNull();
      expect(eventToAccelerator(makeKeyEvent('OS', { ctrl: true }))).toBeNull();
      expect(eventToAccelerator(makeKeyEvent('Win', { ctrl: true }))).toBeNull();
      expect(eventToAccelerator(makeKeyEvent('Hyper', { ctrl: true }))).toBeNull();
    });

    it('Win+Space 生成 Super+Space', () => {
      const event = makeKeyEvent(' ', { meta: true });
      expect(eventToAccelerator(event)).toBe('Super+Space');
    });

    it('Linux 上 metaKey 不置位时，靠 superHeld 兜底补上 Super', () => {
      // 部分 X11 桌面按住 Win 时，主键事件的 metaKey 可能为 false
      const event = makeKeyEvent(' ', { meta: false });
      expect(eventToAccelerator(event, { superHeld: true })).toBe('Super+Space');
      // 没按住 Win 时不应凭空加 Super
      expect(eventToAccelerator(event, { superHeld: false })).toBeNull();
    });

    it('只有修饰键返回 null', () => {
      const event = makeKeyEvent('Control', { ctrl: true });
      expect(eventToAccelerator(event)).toBeNull();
    });

    it('没有修饰键返回 null', () => {
      const event = makeKeyEvent('k');
      expect(eventToAccelerator(event)).toBeNull();
    });

    it('Shift 加字母', () => {
      const event = makeKeyEvent('A', { shift: true });
      expect(eventToAccelerator(event)).toBe('Shift+A');
    });

    it('所有修饰键组合', () => {
      const event = makeKeyEvent('k', {
        ctrl: true,
        alt: true,
        shift: true,
        meta: true,
      });
      expect(eventToAccelerator(event)).toBe('Ctrl+Alt+Shift+Super+K');
    });
  });

  describe('formatAccelerator', () => {
    it('格式化 Ctrl+Alt+K', () => {
      expect(formatAccelerator('Ctrl+Alt+K')).toBe('Ctrl+Alt+K');
    });

    it('格式化 Super 为 Win', () => {
      expect(formatAccelerator('Super+K')).toBe('Win+K');
    });

    it('格式化 Space 为空格', () => {
      expect(formatAccelerator('Alt+Space')).toBe('Alt+空格');
    });

    it('格式化 Return 为 Enter', () => {
      expect(formatAccelerator('Ctrl+Return')).toBe('Ctrl+Enter');
    });

    it('格式化 Escape 为 Esc', () => {
      expect(formatAccelerator('Ctrl+Escape')).toBe('Ctrl+Esc');
    });
  });

  describe('isValidAccelerator', () => {
    it('有效：Ctrl+K', () => {
      expect(isValidAccelerator('Ctrl+K')).toBe(true);
    });

    it('有效：Ctrl+Alt+Delete', () => {
      expect(isValidAccelerator('Ctrl+Alt+Delete')).toBe(true);
    });

    it('无效：空字符串', () => {
      expect(isValidAccelerator('')).toBe(false);
    });

    it('无效：只有主键', () => {
      expect(isValidAccelerator('K')).toBe(false);
    });

    it('无效：只有修饰键', () => {
      expect(isValidAccelerator('Ctrl')).toBe(false);
    });

    it('有效：Shift+Alt+F1', () => {
      expect(isValidAccelerator('Shift+Alt+F1')).toBe(true);
    });
  });
});
