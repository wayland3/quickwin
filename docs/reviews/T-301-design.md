# T-301 mini-design（应用索引）

## 评审级别

TASKS.md 标注 `[L1]`，但本任务跨 `platform/`（COM/Win32）、`services/`、`commands/` 并新增
schema migration（v3），按 `AGENTS.md` §2 #11 上调为 **L2**：设计评审 + 实现评审。

## 改动文件

- `src-tauri/Cargo.toml`：`windows` crate 追加 feature `Win32_Storage_FileSystem`、
  `Win32_UI_Shell_PropertiesSystem`、`Win32_System_Registry`（复用既有依赖，不新增 crate）
- `src-tauri/src/platform/apps.rs`（新增）：COM 初始化守卫、`.lnk` 解析、`App Paths` 注册表读取、
  `shell:AppsFolder` 枚举
- `src-tauri/src/platform/mod.rs`：导出 `apps`
- `src-tauri/src/services/appindex.rs`（新增）：纯函数合并/去重/过滤 + `collect()` 采集编排
- `src-tauri/src/services/mod.rs`：导出 `appindex`
- `src-tauri/src/services/storage.rs`：`SCHEMA_VERSION` 2 → 3、`SCHEMA_V3`（`app_index` 表）、
  `AppIndexRow`、`replace_app_index()`、v2→v3 迁移测试
- `src-tauri/src/commands/launcher.rs`（新增）+ `commands/mod.rs` + `lib.rs`：`app_index_rebuild`
- `src-tauri/src/models.rs`：`AppIndexRebuildResult { count }` + 契约测试
- `src/lib/ipc.ts`、`src/lib/launcher.ts`（新增类型）、`src/lib/contract/AppIndexRebuildResult.keys.json`、
  `src/lib/ipc.contract.test.ts`
- `docs/ARCHITECTURE.md`：§5.2 版本归属补 v3；`TASKS.md` 回写

## 模块交互

```
commands::launcher::app_index_rebuild
  → spawn_blocking（禁止在 async 里做 COM / 长事务）
    → services::appindex::collect()
        ├─ std::fs 遍历开始菜单目录（用户 + 全局），得到 .lnk 路径
        ├─ platform::apps::resolve_shortcuts(paths)      // COM: IShellLinkW + IPersistFile
        ├─ platform::apps::list_app_paths()               // 注册表 App Paths（HKLM/HKCU + WOW6432Node）
        └─ platform::apps::list_uwp_apps()                // COM: shell:AppsFolder + IShellItem2
    → 纯函数 build_index(candidates)：过滤、按 key 小写去重（start_menu > app_paths > uwp）、稳定排序
    → ClipboardStorage::open_write_at → replace_app_index()（单事务 DELETE + INSERT）
  → 返回 { count }
```

数据流单向：`commands → services → platform`。`services` 只做文件系统遍历与合并，所有 Win32/COM 在
`platform/apps.rs`，满足硬规则 #1。

## 数据模型

`app_index` 按 `ARCHITECTURE.md` §5.2 原结构建表（`key UNIQUE`、`name` 索引），
`kind ∈ {win32,uwp,url,command}`、`source ∈ {start_menu,app_paths,uwp,builtin}`。
T-301 只写 `win32`（开始菜单 / App Paths）与 `uwp`；`url`/`command`/`builtin` 留给 T-306。

- `key`：小写可执行文件绝对路径；UWP 为小写 AUMID
- `target`：可执行文件路径；UWP 为 AUMID（启动由 T-304 用 `IApplicationActivationManager`）
- `icon_path`：`.lnk` 的图标位置（`path,index` 形式），无则回退 target。**不做 HICON 提取**
  （需要 WIC/PNG 转换，属启动器 UI 任务）

## 错误处理

- COM 初始化失败 → `AppError::Internal`，command 返回错误（前端统一中文 toast）
- 单个 `.lnk` 解析失败 / 目标不存在 / 非可执行扩展 → 跳过并计入 `failed`，不中断整次重建（只记数量，不记路径）
- 注册表或 UWP 枚举失败 → 降级为空结果 + `warn`（不阻断其他来源），重建仍返回成功
- SQLite 写入失败 → `AppError::Storage`，整次替换回滚（旧索引保留）
- 采集结果为空 → 返回 `count = 0` 并 `warn`，不报错（空开始菜单不应表现为崩溃）

## 测试策略

- 纯函数：去重优先级、重复 key 只留一条、跳过卸载/帮助类快捷方式、目标扩展过滤、key 小写、稳定排序
- 存储：v2 → v3 迁移保留剪贴板与设置；`replace_app_index` 成功 / 幂等重跑 / 空列表清空 / 1000 条耗时
- 契约：`AppIndexRebuildResult` 字段集合前后端共享 fixture
- 手测（`#[ignore]`）：真机扫描，打印各来源条数、失败数与耗时；对照任务验收人工抽查 30 个应用

## 风险与取舍

1. **COM 线程模型**：`spawn_blocking` 线程上 `CoInitializeEx(STA)`，`S_OK`/`S_FALSE` 配对
   `CoUninitialize`，`RPC_E_CHANGED_MODE` 视为已初始化不再反初始化；所有 `unsafe` 带 `// SAFETY:`。
2. **`.lnk` 网络目标**：离线的网络快捷方式可能阻塞；v1 接受，超时策略随 T-302 刷新再评估。
3. **图标**：只存图标位置字符串，不做 HICON→PNG（偏差登记到 TASKS）。
4. **性能**：1000 条数据库替换有测试兜底；COM 扫描耗时靠真机 `#[ignore]` 测试记录，
   若超 2s 在 T-302 做增量刷新时优化。
5. **未含启动刷新**：启动异步刷新与 10 分钟轮询属 T-302，本任务只交付可命令触发的完整重建。

## 设计评审记录（GLM-5.3，2026-10-29，1 轮）

结论：**PASS**。NONBLOCK 3 条及处理见 `docs/reviews/T-301.md`。

