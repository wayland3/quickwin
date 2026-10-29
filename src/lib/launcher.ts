/** 启动器相关共享类型（与 Rust `models.rs` 对齐） */

/** `app_index_rebuild` 返回值 */
export interface AppIndexRebuildResult {
  /** 写入的应用条数 */
  count: number;
}

/** 结果类型 */
export type LauncherKind = 'app' | 'file' | 'command' | 'web';

/** 单条启动器结果 */
export interface LauncherItem {
  key: string;
  title: string;
  subtitle: string | null;
  kind: LauncherKind;
  icon: string | null;
  score: number;
}

/** `launcher_query` 返回值 */
export interface LauncherQueryResult {
  items: LauncherItem[];
  hint: string | null;
}
