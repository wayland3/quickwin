import { describe, expect, it } from 'vitest';
import type {
  ClipboardDetail,
  ClipboardItem,
  ClipboardListResult,
  Settings,
} from './clipboard';
import type { AppIndexRebuildResult, LauncherItem, LauncherQueryResult } from './launcher';

import ClipboardItemKeys from './contract/ClipboardItem.keys.json';
import ClipboardDetailKeys from './contract/ClipboardDetail.keys.json';
import ClipboardListResultKeys from './contract/ClipboardListResult.keys.json';
import SettingsKeys from './contract/Settings.keys.json';
import AppIndexRebuildResultKeys from './contract/AppIndexRebuildResult.keys.json';
import LauncherItemKeys from './contract/LauncherItem.keys.json';
import LauncherQueryResultKeys from './contract/LauncherQueryResult.keys.json';

/**
 * IPC 契约测试
 *
 * 验证 TS 类型字段集合与 Rust 序列化结果一致。
 * 字段名列表存储在 src/lib/contract/*.keys.json，Rust 侧测试读取同一份 fixture 比对。
 * 任一侧删除/重命名字段都会导致测试失败。
 */

describe('IPC 契约', () => {
  describe('ClipboardItem 字段集合', () => {
    it('与 Rust 序列化一致', () => {
      const item: ClipboardItem = {
        id: 1,
        kind: 'text',
        preview: 'test',
        source_app: null,
        created_at: 0,
        last_copied_at: 0,
        copy_count: 1,
        pinned: false,
        sensitive: false,
        truncated: false,
        byte_size: 0,
        thumb_path: null,
      };

      const keys = Object.keys(item).sort();
      expect(keys).toEqual(ClipboardItemKeys.sort());
    });
  });

  describe('ClipboardDetail 字段集合', () => {
    it('与 Rust 序列化一致', () => {
      const detail: ClipboardDetail = {
        id: 1,
        kind: 'text',
        preview: 'test',
        text_body: null,
        html: null,
        rtf: null,
        file_list: null,
        file_exists: null,
        blob_path: null,
        thumb_path: null,
        byte_size: 0,
        created_at: 0,
        last_copied_at: 0,
        copy_count: 1,
        pinned: false,
        truncated: false,
        source_app: null,
      };

      const keys = Object.keys(detail).sort();
      expect(keys).toEqual(ClipboardDetailKeys.sort());
    });
  });

  describe('ClipboardListResult 字段集合', () => {
    it('与 Rust 序列化一致', () => {
      const result: ClipboardListResult = {
        items: [],
        total: 0,
      };

      const keys = Object.keys(result).sort();
      expect(keys).toEqual(ClipboardListResultKeys.sort());
    });
  });

  describe('Settings 字段集合', () => {
    it('与 Rust 序列化一致', () => {
      const settings: Settings = {
        launcher_hotkey: 'Alt+Space',
        clipboard_hotkey: 'Alt+V',
        retention_days: 15,
        max_items: 10000,
        record_text: true,
        record_image: true,
        record_files: true,
        excluded_apps: [],
        theme: 'system',
        autostart: false,
        pause_recording: false,
        restore_clipboard_after_paste: false,
      };

      const keys = Object.keys(settings).sort();
      expect(keys).toEqual(SettingsKeys.sort());
    });
  });

  describe('AppIndexRebuildResult 字段集合', () => {
    it('与 Rust 序列化一致', () => {
      const result: AppIndexRebuildResult = { count: 0 };

      const keys = Object.keys(result).sort();
      expect(keys).toEqual(AppIndexRebuildResultKeys.sort());
    });
  });

  describe('LauncherItem 字段集合', () => {
    it('与 Rust 序列化一致', () => {
      const item: LauncherItem = {
        key: 'a.exe',
        title: 'Alpha',
        subtitle: null,
        kind: 'app',
        icon: null,
        score: 1,
      };

      const keys = Object.keys(item).sort();
      expect(keys).toEqual(LauncherItemKeys.sort());
    });
  });

  describe('LauncherQueryResult 字段集合', () => {
    it('与 Rust 序列化一致', () => {
      const result: LauncherQueryResult = { items: [], hint: null };

      const keys = Object.keys(result).sort();
      expect(keys).toEqual(LauncherQueryResultKeys.sort());
    });
  });
});
