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
};
