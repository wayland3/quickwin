import { useCallback, useEffect, useState } from 'react';
import { ipc } from '../lib/ipc';
import type { HotkeyProbeResult } from '../lib/ipc';
import type { Settings } from '../lib/clipboard';
import { HotkeyRecorder } from '../components/HotkeyRecorder';

type SettingsTab = 'general' | 'hotkeys' | 'clipboard' | 'privacy' | 'about';

const TABS: { id: SettingsTab; label: string }[] = [
  { id: 'general', label: '常规' },
  { id: 'hotkeys', label: '热键' },
  { id: 'clipboard', label: '剪贴板' },
  { id: 'privacy', label: '隐私' },
  { id: 'about', label: '关于' },
];

export function SettingsPanel() {
  const [activeTab, setActiveTab] = useState<SettingsTab>('general');
  const [settings, setSettings] = useState<Settings | null>(null);
  const [saving, setSaving] = useState(false);
  const [error, setError] = useState<string | null>(null);

  useEffect(() => {
    ipc.settingsGet()
      .then(setSettings)
      .catch((err) => setError(String(err)));
  }, []);

  const updateSetting = async <K extends keyof Settings>(
    key: K,
    value: Settings[K],
  ) => {
    if (!settings) return;
    
    setSaving(true);
    setError(null);
    
    try {
      const updated = await ipc.settingsUpdate({ [key]: value });
      setSettings(updated);
    } catch (err) {
      setError(String(err));
    } finally {
      setSaving(false);
    }
  };

  if (!settings) {
    return (
      <div className="flex h-full items-center justify-center">
        <div className="text-slate-400">加载中...</div>
      </div>
    );
  }

  return (
    <div className="flex h-full bg-[var(--bg)]">
      {/* 左侧导航 */}
      <nav className="flex w-[200px] flex-none flex-col border-r border-[var(--border)] bg-[var(--surface)] p-4">
        <h1 className="mb-6 text-lg font-semibold text-[var(--text)]">设置</h1>
        {TABS.map((tab) => (
          <button
            key={tab.id}
            onClick={() => setActiveTab(tab.id)}
            className={`mb-2 rounded-lg px-4 py-2 text-left text-sm transition-colors ${
              activeTab === tab.id
                ? 'bg-[var(--accent)] text-white'
                : 'text-[var(--text-muted)] hover:bg-[var(--surface-hover)]'
            }`}
          >
            {tab.label}
          </button>
        ))}
      </nav>

      {/* 右侧内容 */}
      <div className="flex flex-1 flex-col overflow-y-auto p-8">
        {error && (
          <div className="mb-4 rounded-lg bg-red-500/10 px-4 py-3 text-sm text-red-500">
            {error}
          </div>
        )}

        {saving && (
          <div className="mb-4 rounded-lg bg-blue-500/10 px-4 py-3 text-sm text-blue-500">
            保存中...
          </div>
        )}

        {activeTab === 'general' && (
          <GeneralTab settings={settings} onUpdate={updateSetting} />
        )}
        {activeTab === 'hotkeys' && (
          <HotkeysTab settings={settings} onUpdate={updateSetting} />
        )}
        {activeTab === 'clipboard' && (
          <ClipboardTab settings={settings} onUpdate={updateSetting} />
        )}
        {activeTab === 'privacy' && (
          <PrivacyTab />
        )}
        {activeTab === 'about' && <AboutTab />}
      </div>
    </div>
  );
}

function GeneralTab({
  settings,
  onUpdate,
}: {
  settings: Settings;
  onUpdate: <K extends keyof Settings>(key: K, value: Settings[K]) => void;
}) {
  const [confirmReset, setConfirmReset] = useState(false);

  const handleReset = async () => {
    if (!confirmReset) {
      setConfirmReset(true);
      return;
    }
    
    try {
      await ipc.settingsReset();
      setConfirmReset(false);
    } catch (err) {
      console.error('恢复默认失败:', err);
    }
  };

  return (
    <div className="max-w-[560px]">
      <h2 className="mb-6 text-base font-semibold text-[var(--text)]">常规设置</h2>
      
      <div className="space-y-6">
        {/* 开机自启 */}
        <div className="flex items-start justify-between gap-4">
          <div className="flex-1">
            <div className="text-sm font-medium text-[var(--text)]">开机自启</div>
            <div className="mt-1 text-xs text-[var(--text-muted)]">
              启动时自动运行 QuickWin
            </div>
          </div>
          <label className="relative inline-flex cursor-pointer items-center">
            <input
              type="checkbox"
              checked={settings.autostart}
              onChange={(e) => onUpdate('autostart', e.target.checked)}
              className="peer sr-only"
            />
            <div className="peer h-6 w-11 rounded-full bg-[var(--surface-active)] after:absolute after:left-[2px] after:top-[2px] after:h-5 after:w-5 after:rounded-full after:border after:border-[var(--border)] after:bg-white after:transition-all after:content-[''] peer-checked:bg-[var(--accent)] peer-checked:after:translate-x-full peer-checked:after:border-white"></div>
          </label>
        </div>

        {/* 主题 */}
        <div className="flex items-start justify-between gap-4">
          <div className="flex-1">
            <div className="text-sm font-medium text-[var(--text)]">主题</div>
            <div className="mt-1 text-xs text-[var(--text-muted)]">
              选择界面配色方案
            </div>
          </div>
          <select
            value={settings.theme}
            onChange={(e) => onUpdate('theme', e.target.value as Settings['theme'])}
            className="rounded-lg border border-[var(--border)] bg-[var(--surface)] px-3 py-1.5 text-sm text-[var(--text)] focus:border-[var(--accent)] focus:outline-none"
          >
            <option value="system">跟随系统</option>
            <option value="light">浅色</option>
            <option value="dark">深色</option>
          </select>
        </div>

        {/* 恢复默认 */}
        <div className="rounded-lg border border-[var(--border)] p-4">
          <div className="mb-3 text-sm font-medium text-[var(--text)]">恢复默认设置</div>
          <div className="mb-3 text-xs text-[var(--text-muted)]">
            将所有设置恢复为默认值（不影响剪贴板历史）
          </div>
          <button
            onClick={handleReset}
            className="rounded-lg bg-red-500 px-4 py-2 text-sm text-white hover:bg-red-600"
          >
            {confirmReset ? '确认恢复' : '恢复默认'}
          </button>
          {confirmReset && (
            <button
              onClick={() => setConfirmReset(false)}
              className="ml-2 rounded-lg border border-[var(--border)] px-4 py-2 text-sm text-[var(--text-muted)] hover:bg-[var(--surface-hover)]"
            >
              取消
            </button>
          )}
        </div>
      </div>
    </div>
  );
}

function HotkeysTab({
  settings,
  onUpdate,
}: {
  settings: Settings;
  onUpdate: <K extends keyof Settings>(key: K, value: Settings[K]) => void;
}) {
  const handleLauncherSave = async (value: string) => {
    await onUpdate('launcher_hotkey', value);
  };

  const handleClipboardSave = async (value: string) => {
    await onUpdate('clipboard_hotkey', value);
  };

  return (
    <div className="max-w-[560px]">
      <h2 className="mb-6 text-base font-semibold text-[var(--text)]">热键设置</h2>
      
      <div className="space-y-6">
        {/* 启动器热键 */}
        <div className="flex items-start justify-between gap-4">
          <div className="flex-1">
            <div className="text-sm font-medium text-[var(--text)]">启动器</div>
            <div className="mt-1 text-xs text-[var(--text-muted)]">
              打开应用启动器
            </div>
            <HotkeyAvailability accelerator={settings.launcher_hotkey} />
          </div>
          <HotkeyRecorder
            value={settings.launcher_hotkey}
            action="launcher"
            onSave={handleLauncherSave}
            placeholder="点击录制"
          />
        </div>

        {/* 剪贴板热键 */}
        <div className="flex items-start justify-between gap-4">
          <div className="flex-1">
            <div className="text-sm font-medium text-[var(--text)]">剪贴板历史</div>
            <div className="mt-1 text-xs text-[var(--text-muted)]">
              打开剪贴板历史面板
            </div>
            <HotkeyAvailability accelerator={settings.clipboard_hotkey} />
          </div>
          <HotkeyRecorder
            value={settings.clipboard_hotkey}
            action="clipboard"
            onSave={handleClipboardSave}
            placeholder="点击录制"
          />
        </div>

        {/* 提示 */}
        <div className="rounded-lg bg-blue-500/10 px-4 py-3 text-xs text-blue-600">
          提示：点击按钮后按下组合键即可录制。按 Esc 取消，按 Backspace 清空。
        </div>
        <div className="rounded-lg bg-blue-500/10 px-4 py-3 text-xs text-blue-600">
          含 Win 键的组合（如 Win+空格）也可以录制：按住 Win 再按主键即可，
          录制过程中系统可能短暂切走焦点，属正常现象。
        </div>
      </div>
    </div>
  );
}

/**
 * 热键可用性实测结果
 *
 * 进入热键页时自动探测一次；点击「重新检测」可再次探测。
 * 探测的是「该组合键此刻能否被注册」，用来区分：
 * - 当前可用
 * - 已被系统或其他程序占用（如桌面环境的 Win+空格）
 * - 格式无效
 */
function HotkeyAvailability({ accelerator }: { accelerator: string }) {
  const [result, setResult] = useState<HotkeyProbeResult | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [checking, setChecking] = useState(false);

  const check = useCallback(async () => {
    setChecking(true);
    setError(null);
    try {
      const r = await ipc.hotkeyProbe(accelerator);
      setResult(r);
    } catch (err) {
      // 探测环境故障：后端直接报错，这里展示可操作的中文提示
      setResult(null);
      setError(err instanceof Error ? err.message : String(err));
    } finally {
      setChecking(false);
    }
  }, [accelerator]);

  // 进入页面自动探测一次；accelerator 变化时重新探测。
  // useEffect 依赖 [check]，check 依赖 [accelerator]，因此 accelerator 变化才触发，
  // 不会形成循环（check 内部只 setState，不改 accelerator）。
  useEffect(() => {
    if (!accelerator) return;
    void check();
  }, [check, accelerator]);

  if (!accelerator) {
    return null;
  }

  const tone = error
    ? 'text-red-500'
    : result?.reason === 'available'
      ? 'text-green-600'
      : 'text-red-500';

  return (
    <div className="mt-1 flex items-center gap-2 text-xs">
      {checking ? (
        <span className="text-[var(--text-muted)]">检测中...</span>
      ) : (
        <span className={tone}>{error ?? result?.message ?? '无法检测'}</span>
      )}
      <button
        type="button"
        onClick={() => void check()}
        className="text-[var(--text-muted)] underline hover:text-[var(--text)]"
      >
        重新检测
      </button>
    </div>
  );
}

function ClipboardTab({
  settings,
  onUpdate,
}: {
  settings: Settings;
  onUpdate: <K extends keyof Settings>(key: K, value: Settings[K]) => void;
}) {
  return (
    <div className="max-w-[560px]">
      <h2 className="mb-6 text-base font-semibold text-[var(--text)]">剪贴板设置</h2>
      
      <div className="space-y-6">
        {/* 记录文本 */}
        <div className="flex items-start justify-between gap-4">
          <div className="flex-1">
            <div className="text-sm font-medium text-[var(--text)]">记录文本</div>
            <div className="mt-1 text-xs text-[var(--text-muted)]">
              保存复制的文本内容（含富文本）
            </div>
          </div>
          <label className="relative inline-flex cursor-pointer items-center">
            <input
              type="checkbox"
              checked={settings.record_text}
              onChange={(e) => onUpdate('record_text', e.target.checked)}
              className="peer sr-only"
            />
            <div className="peer h-6 w-11 rounded-full bg-[var(--surface-active)] after:absolute after:left-[2px] after:top-[2px] after:h-5 after:w-5 after:rounded-full after:border after:border-[var(--border)] after:bg-white after:transition-all after:content-[''] peer-checked:bg-[var(--accent)] peer-checked:after:translate-x-full peer-checked:after:border-white"></div>
          </label>
        </div>

        {/* 记录图片 */}
        <div className="flex items-start justify-between gap-4">
          <div className="flex-1">
            <div className="text-sm font-medium text-[var(--text)]">记录图片</div>
            <div className="mt-1 text-xs text-[var(--text-muted)]">
              保存复制的图片
            </div>
          </div>
          <label className="relative inline-flex cursor-pointer items-center">
            <input
              type="checkbox"
              checked={settings.record_image}
              onChange={(e) => onUpdate('record_image', e.target.checked)}
              className="peer sr-only"
            />
            <div className="peer h-6 w-11 rounded-full bg-[var(--surface-active)] after:absolute after:left-[2px] after:top-[2px] after:h-5 after:w-5 after:rounded-full after:border after:border-[var(--border)] after:bg-white after:transition-all after:content-[''] peer-checked:bg-[var(--accent)] peer-checked:after:translate-x-full peer-checked:after:border-white"></div>
          </label>
        </div>

        {/* 记录文件列表 */}
        <div className="flex items-start justify-between gap-4">
          <div className="flex-1">
            <div className="text-sm font-medium text-[var(--text)]">记录文件列表</div>
            <div className="mt-1 text-xs text-[var(--text-muted)]">
              保存复制的文件路径列表
            </div>
          </div>
          <label className="relative inline-flex cursor-pointer items-center">
            <input
              type="checkbox"
              checked={settings.record_files}
              onChange={(e) => onUpdate('record_files', e.target.checked)}
              className="peer sr-only"
            />
            <div className="peer h-6 w-11 rounded-full bg-[var(--surface-active)] after:absolute after:left-[2px] after:top-[2px] after:h-5 after:w-5 after:rounded-full after:border after:border-[var(--border)] after:bg-white after:transition-all after:content-[''] peer-checked:bg-[var(--accent)] peer-checked:after:translate-x-full peer-checked:after:border-white"></div>
          </label>
        </div>

        {/* 保留天数 */}
        <div className="flex items-start justify-between gap-4">
          <div className="flex-1">
            <div className="text-sm font-medium text-[var(--text)]">保留天数</div>
            <div className="mt-1 text-xs text-[var(--text-muted)]">
              自动清理超过指定天数的记录
            </div>
          </div>
          <select
            value={settings.retention_days}
            onChange={(e) => onUpdate('retention_days', Number(e.target.value))}
            className="rounded-lg border border-[var(--border)] bg-[var(--surface)] px-3 py-1.5 text-sm text-[var(--text)] focus:border-[var(--accent)] focus:outline-none"
          >
            <option value={7}>7 天</option>
            <option value={15}>15 天</option>
            <option value={30}>30 天</option>
            <option value={90}>90 天</option>
            <option value={-1}>永不</option>
          </select>
        </div>

        {/* 条数上限 */}
        <div className="flex items-start justify-between gap-4">
          <div className="flex-1">
            <div className="text-sm font-medium text-[var(--text)]">条数上限</div>
            <div className="mt-1 text-xs text-[var(--text-muted)]">
              最多保存的记录数量（1000-50000）
            </div>
          </div>
          <MaxItemsInput settings={settings} onUpdate={onUpdate} />
        </div>

        {/* 粘贴后还原 */}
        <div className="flex items-start justify-between gap-4">
          <div className="flex-1">
            <div className="text-sm font-medium text-[var(--text)]">粘贴后还原原剪贴板</div>
            <div className="mt-1 text-xs text-[var(--text-muted)]">
              粘贴后恢复剪贴板到粘贴前的内容
            </div>
          </div>
          <label className="relative inline-flex cursor-pointer items-center">
            <input
              type="checkbox"
              checked={settings.restore_clipboard_after_paste}
              onChange={(e) => onUpdate('restore_clipboard_after_paste', e.target.checked)}
              className="peer sr-only"
            />
            <div className="peer h-6 w-11 rounded-full bg-[var(--surface-active)] after:absolute after:left-[2px] after:top-[2px] after:h-5 after:w-5 after:rounded-full after:border after:border-[var(--border)] after:bg-white after:transition-all after:content-[''] peer-checked:bg-[var(--accent)] peer-checked:after:translate-x-full peer-checked:after:border-white"></div>
          </label>
        </div>
      </div>
    </div>
  );
}

function MaxItemsInput({
  settings,
  onUpdate,
}: {
  settings: Settings;
  onUpdate: <K extends keyof Settings>(key: K, value: Settings[K]) => void;
}) {
  const [draft, setDraft] = useState(String(settings.max_items));
  const [error, setError] = useState<string | null>(null);

  const commit = async () => {
    const value = Number(draft);
    if (value === settings.max_items) return;
    if (isNaN(value) || value < 1000 || value > 50000) {
      setError('请输入 1000-50000 之间的数字');
      return;
    }
    try {
      await onUpdate('max_items', value);
      setError(null);
    } catch (err) {
      setError(err instanceof Error ? err.message : String(err));
    }
  };

  return (
    <div>
      <input
        type="number"
        value={draft}
        onChange={(e) => setDraft(e.target.value)}
        onBlur={commit}
        onKeyDown={(e) => {
          if (e.key === 'Enter') commit();
        }}
        min={1000}
        max={50000}
        className="w-24 rounded-lg border border-[var(--border)] bg-[var(--surface)] px-3 py-1.5 text-sm text-[var(--text)] focus:border-[var(--accent)] focus:outline-none"
      />
      {error && <div className="mt-1 text-xs text-red-500">{error}</div>}
    </div>
  );
}

function PrivacyTab() {
  const [confirmClear, setConfirmClear] = useState(false);

  const handleClearHistory = async () => {
    if (!confirmClear) {
      setConfirmClear(true);
      return;
    }
    
    try {
      await ipc.clipboardClear(true); // include_pinned
      setConfirmClear(false);
    } catch (err) {
      console.error('清空历史失败:', err);
    }
  };

  return (
    <div className="max-w-[560px]">
      <h2 className="mb-6 text-base font-semibold text-[var(--text)]">隐私设置</h2>
      
      <div className="space-y-6">
        {/* 清空历史 */}
        <div className="rounded-lg border border-[var(--border)] p-4">
          <div className="mb-3 text-sm font-medium text-[var(--text)]">清空历史记录</div>
          <div className="mb-3 text-xs text-[var(--text-muted)]">
            删除所有剪贴板历史记录（包括置顶项）
          </div>
          <button
            onClick={handleClearHistory}
            className="rounded-lg bg-red-500 px-4 py-2 text-sm text-white hover:bg-red-600"
          >
            {confirmClear ? '确认清空' : '清空历史'}
          </button>
          {confirmClear && (
            <button
              onClick={() => setConfirmClear(false)}
              className="ml-2 rounded-lg border border-[var(--border)] px-4 py-2 text-sm text-[var(--text-muted)] hover:bg-[var(--surface-hover)]"
            >
              取消
            </button>
          )}
        </div>

        <div className="rounded-lg bg-blue-500/10 px-4 py-3 text-xs text-blue-600">
          提示：剪贴板数据仅本地存储，不会上传到云端。
        </div>
      </div>
    </div>
  );
}

function AboutTab() {
  return (
    <div className="max-w-[560px]">
      <h2 className="mb-6 text-base font-semibold text-[var(--text)]">关于</h2>
      
      <div className="space-y-4">
        <div className="rounded-lg border border-[var(--border)] p-4">
          <div className="mb-2 text-sm font-medium text-[var(--text)]">QuickWin</div>
          <div className="text-xs text-[var(--text-muted)]">版本 0.1.0</div>
        </div>

        <div className="rounded-lg border border-[var(--border)] p-4">
          <div className="mb-2 text-sm font-medium text-[var(--text)]">数据目录</div>
          <div className="text-xs text-[var(--text-muted)]">
            %APPDATA%\com.quickwin.app
          </div>
        </div>

        <div className="rounded-lg border border-[var(--border)] p-4">
          <div className="mb-2 text-sm font-medium text-[var(--text)]">技术栈</div>
          <div className="text-xs text-[var(--text-muted)]">
            Tauri 2 + Rust + React 19 + TypeScript
          </div>
        </div>

        <div className="rounded-lg bg-slate-500/10 px-4 py-3 text-xs text-slate-600">
          QuickWin 是一个 Windows 常驻效率工具，提供启动器和剪贴板历史功能。
        </div>
      </div>
    </div>
  );
}
