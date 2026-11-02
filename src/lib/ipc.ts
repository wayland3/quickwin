import { invoke } from '@tauri-apps/api/core';
import type {
  ClipFilter,
  ClipboardDetail,
  ClipboardListResult,
  Settings,
} from './clipboard';
import type { AppIndexRebuildResult, LauncherQueryResult } from './launcher';

/** 热键可用性实测结果（对应 Rust `HotkeyProbeResult`） */
export interface HotkeyProbeResult {
  accelerator: string;
  available: boolean;
  reason: 'available' | 'occupied' | 'invalid';
  message: string;
}

/** 后端错误结构 */
interface AppError {
  code: string;
  message: string;
}

/** 错误码 → 中文 toast 映射 */
const ERROR_MESSAGES: Record<string, string> = {
  'clipboard.busy': '剪贴板暂时被占用，请稍后重试',
  'clipboard.read_failed': '无法读取剪贴板内容',
  'focus.elevated_target': '目标窗口权限较高，无法自动操作',
  'content.too_large': '内容过长（超过 256KB）',
  'hotkey.conflict': '热键已被其他程序占用',
  'hotkey.register_failed': '热键注册失败，请尝试更换',
  'storage.error': '数据库操作失败',
  'settings.invalid': '设置值不合法',
  'launcher.launch_failed': '无法打开该应用，请确认它仍然安装',
  internal: '发生内部错误，请查看日志',
};

/** 统一错误处理：将后端错误转为中文 toast */
function handleError(err: unknown, command?: string): never {
  if (err && typeof err === 'object' && 'code' in err) {
    const appError = err as AppError;
    // 打开应用失败：后端文案已按原因分类（已卸载 / 无权限 / 无关联程序），直接展示
    if (
      appError.code === 'launcher.launch_failed' &&
      typeof appError.message === 'string' &&
      appError.message
    ) {
      throw new Error(appError.message);
    }
    // 热键探测失败：后端已给出可操作的中文原因，直接展示
    if (
      appError.code === 'hotkey.probe_failed' &&
      typeof appError.message === 'string' &&
      appError.message
    ) {
      throw new Error(appError.message);
    }
    const message = ERROR_MESSAGES[appError.code];
    if (message) {
      throw new Error(message);
    }
    // 未知错误码：记录但不泄露后端 message
    console.error('[ipc] unhandled error code:', appError.code, 'command:', command);
    throw new Error('操作失败，请稍后重试');
  }
  // 非 AppError 异常：不泄露内部细节
  throw new Error('操作失败，请稍后重试');
}

/** 包装 invoke 调用，统一错误处理 */
async function invokeWrapper<T>(command: string, args?: Record<string, unknown>): Promise<T> {
  try {
    return await invoke<T>(command, args);
  } catch (err) {
    return handleError(err, command);
  }
}

export const ipc = {
  async appPing(): Promise<string> {
    return invokeWrapper<string>('app_ping');
  },

  async autostartEnable(): Promise<void> {
    return invokeWrapper<void>('autostart_enable');
  },

  async autostartDisable(): Promise<void> {
    return invokeWrapper<void>('autostart_disable');
  },

  async autostartIsEnabled(): Promise<boolean> {
    return invokeWrapper<boolean>('autostart_is_enabled');
  },

  async overlayShow(mode: 'launcher' | 'clipboard'): Promise<void> {
    return invokeWrapper<void>('overlay_show', { payload: { mode } });
  },

  async overlayHide(): Promise<void> {
    return invokeWrapper<void>('overlay_hide');
  },

  async overlayResize(height: number): Promise<void> {
    return invokeWrapper<void>('overlay_resize', { payload: { height } });
  },

  async hotkeyStatus(): Promise<{
    bindings: Array<{ action: string; accelerator: string }>;
    failed: Array<{ action: string; accelerator: string; code: string }>;
  }> {
    return invokeWrapper('hotkey_status');
  },

  /** 实测某组合键当前是否可用（可用 / 被占用 / 格式无效） */
  async hotkeyProbe(accelerator: string): Promise<HotkeyProbeResult> {
    return invokeWrapper<HotkeyProbeResult>('hotkey_probe', { accelerator });
  },

  async hotkeyRebind(
    action: 'launcher' | 'clipboard',
    accelerator: string,
  ): Promise<boolean> {
    return invokeWrapper<boolean>('hotkey_rebind', { action, accelerator });
  },

  async hotkeySuspend(action: 'launcher' | 'clipboard'): Promise<void> {
    return invokeWrapper<void>('hotkey_suspend', { action });
  },

  async hotkeyResume(action: 'launcher' | 'clipboard'): Promise<void> {
    return invokeWrapper<void>('hotkey_resume', { action });
  },

  async appIndexRebuild(): Promise<AppIndexRebuildResult> {
    return invokeWrapper<AppIndexRebuildResult>('app_index_rebuild');
  },

  async launcherQuery(query: string, limit?: number): Promise<LauncherQueryResult> {
    return invokeWrapper<LauncherQueryResult>('launcher_query', { query, limit });
  },

  async launcherExecute(key: string): Promise<void> {
    return invokeWrapper<void>('launcher_execute', { key });
  },

  async clipboardList(params: {
    query: string;
    filter: ClipFilter;
    limit: number;
    offset: number;
  }): Promise<ClipboardListResult> {
    return invokeWrapper<ClipboardListResult>('clipboard_list', params);
  },

  async clipboardGet(id: number): Promise<ClipboardDetail> {
    return invokeWrapper<ClipboardDetail>('clipboard_get', { id });
  },

  async clipboardSetPinned(id: number, pinned: boolean): Promise<void> {
    return invokeWrapper<void>('clipboard_set_pinned', { id, pinned });
  },

  async clipboardDelete(id: number, hard: boolean): Promise<void> {
    return invokeWrapper<void>('clipboard_delete', { id, hard });
  },

  async clipboardClear(includePinned: boolean): Promise<void> {
    return invokeWrapper<void>('clipboard_clear', { includePinned });
  },

  async clipboardActivate(id: number, mode: 'paste' | 'copy'): Promise<void> {
    return invokeWrapper<void>('clipboard_activate', { id, mode });
  },

  async clipboardSetRestoreAfterPaste(enabled: boolean): Promise<void> {
    return invokeWrapper<void>('clipboard_set_restore_after_paste', { enabled });
  },

  async settingsGet(): Promise<Settings> {
    return invokeWrapper<Settings>('settings_get');
  },

  async settingsUpdate(patch: Partial<Settings>): Promise<Settings> {
    return invokeWrapper<Settings>('settings_update', { patch });
  },

  async settingsReset(): Promise<Settings> {
    return invokeWrapper<Settings>('settings_reset');
  },
};
