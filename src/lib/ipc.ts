import { invoke } from '@tauri-apps/api/core';

export const ipc = {
  async appPing(): Promise<string> {
    return invoke<string>('app_ping');
  },
};
