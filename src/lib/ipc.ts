import { invoke } from '@tauri-apps/api/core';
import type {
  ClipFilter,
  ClipboardDetail,
  ClipboardListResult,
} from './clipboard';

export const ipc = {
  async appPing(): Promise<string> {
    return invoke<string>('app_ping');
  },

  async autostartEnable(): Promise<void> {
    return invoke<void>('autostart_enable');
  },

  async autostartDisable(): Promise<void> {
    return invoke<void>('autostart_disable');
  },

  async autostartIsEnabled(): Promise<boolean> {
    return invoke<boolean>('autostart_is_enabled');
  },

  async overlayShow(mode: string): Promise<void> {
    return invoke<void>('overlay_show', { payload: { mode } });
  },

  async overlayHide(): Promise<void> {
    return invoke<void>('overlay_hide');
  },

  async overlayResize(height: number): Promise<void> {
    return invoke<void>('overlay_resize', { payload: { height } });
  },

  async hotkeyStatus(): Promise<{
    bindings: Array<{ action: string; accelerator: string }>;
    failed: Array<{ action: string; accelerator: string; code: string }>;
  }> {
    return invoke('hotkey_status');
  },

  async hotkeyRebind(action: string, accelerator: string): Promise<boolean> {
    return invoke<boolean>('hotkey_rebind', { action, accelerator });
  },

  async clipboardList(params: {
    query: string;
    filter: ClipFilter;
    limit: number;
    offset: number;
  }): Promise<ClipboardListResult> {
    return invoke<ClipboardListResult>('clipboard_list', params);
  },

  async clipboardGet(id: number): Promise<ClipboardDetail> {
    return invoke<ClipboardDetail>('clipboard_get', { id });
  },

  async clipboardSetPinned(id: number, pinned: boolean): Promise<void> {
    return invoke<void>('clipboard_set_pinned', { id, pinned });
  },

  async clipboardDelete(id: number, hard: boolean): Promise<void> {
    return invoke<void>('clipboard_delete', { id, hard });
  },

  async clipboardActivate(id: number, mode: 'paste' | 'copy'): Promise<void> {
    return invoke<void>('clipboard_activate', { id, mode });
  },

  async clipboardSetRestoreAfterPaste(enabled: boolean): Promise<void> {
    return invoke<void>('clipboard_set_restore_after_paste', { enabled });
  },
};
