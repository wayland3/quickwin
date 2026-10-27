import { invoke } from '@tauri-apps/api/core';

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
};
