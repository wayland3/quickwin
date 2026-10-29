//! 应用索引采集与合并。
//!
//! 职责边界：
//! - 文件系统遍历（开始菜单目录）与候选合并在此；
//! - 快捷方式解析 / 注册表 / UWP 枚举在 `platform::apps`；
//! - 落库在 `services::storage::replace_app_index`。

use crate::error::AppError;
use crate::platform::apps::{self, AppPathEntry, ResolvedShortcut, UwpApp};
use crate::services::storage::{AppIndexRow, ClipboardStorage};
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, MutexGuard};
use std::thread::JoinHandle;
use std::time::Duration;

/// 来源标识
pub const SOURCE_START_MENU: &str = "start_menu";
pub const SOURCE_APP_PATHS: &str = "app_paths";
pub const SOURCE_UWP: &str = "uwp";

/// 类型标识
pub const KIND_WIN32: &str = "win32";
pub const KIND_UWP: &str = "uwp";

/// 开始菜单目录最大递归深度（正常结构 ≤3）
const MAX_DIR_DEPTH: usize = 4;

/// 来源优先级：同 key 时保留优先级高者（值越小越优先）
fn source_priority(source: &str) -> u8 {
    match source {
        SOURCE_START_MENU => 0,
        SOURCE_APP_PATHS => 1,
        SOURCE_UWP => 2,
        _ => 3,
    }
}

/// 非应用类快捷方式名称特征（卸载/说明类，不计入索引）
const NON_APP_NAME_HINTS: [&str; 5] = ["uninstall", "卸载", "readme", "帮助", "help"];

/// 可作为启动目标的可执行扩展名
const EXECUTABLE_EXTENSIONS: [&str; 4] = ["exe", "bat", "cmd", "com"];

/// 采集候选（过滤前）
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AppCandidate {
    pub name: String,
    /// 可执行文件路径或 AUMID
    pub target: String,
    pub args: Option<String>,
    pub icon_path: Option<String>,
    pub kind: String,
    pub source: String,
}

/// 采集统计（不含任何路径或应用名，可安全写日志）
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct ScanStats {
    /// 扫描到的 .lnk 文件数
    pub start_menu_links: usize,
    /// 解析失败或目标不可用的快捷方式数
    pub start_menu_failed: usize,
    /// App Paths 候选数
    pub app_paths: usize,
    /// UWP 候选数
    pub uwp: usize,
    /// UWP 枚举是否整体失败（失败时降级为空）
    pub uwp_failed: bool,
}

/// 采集结果
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AppIndexScan {
    pub rows: Vec<AppIndexRow>,
    pub stats: ScanStats,
}

/// 合并候选：过滤非应用、按 key 去重、稳定排序
pub fn build_index(candidates: Vec<AppCandidate>) -> Vec<AppIndexRow> {
    let mut chosen: HashMap<String, AppCandidate> = HashMap::new();

    for candidate in candidates {
        let key = normalize_key(&candidate.target);
        if key.is_empty() || candidate.name.trim().is_empty() || !is_usable(&candidate) {
            continue;
        }

        match chosen.get(&key) {
            Some(existing) if prefer(existing, &candidate) => {}
            _ => {
                chosen.insert(key, candidate);
            }
        }
    }

    let mut rows: Vec<AppIndexRow> = chosen
        .into_iter()
        .map(|(key, candidate)| AppIndexRow {
            key,
            name: candidate.name.trim().to_string(),
            target: candidate.target,
            args: candidate.args,
            kind: candidate.kind,
            source: candidate.source,
            icon_path: candidate.icon_path,
        })
        .collect();

    rows.sort_by(|left, right| {
        source_priority(&left.source)
            .cmp(&source_priority(&right.source))
            .then_with(|| left.name.to_lowercase().cmp(&right.name.to_lowercase()))
            .then_with(|| left.key.cmp(&right.key))
    });

    rows
}

/// 去重键：路径 / AUMID 一律小写
pub fn normalize_key(target: &str) -> String {
    target.trim().to_lowercase()
}

/// 已有候选是否比新候选更该保留
fn prefer(existing: &AppCandidate, incoming: &AppCandidate) -> bool {
    let existing_priority = source_priority(&existing.source);
    let incoming_priority = source_priority(&incoming.source);
    if existing_priority != incoming_priority {
        return existing_priority < incoming_priority;
    }

    let existing_name = existing.name.to_lowercase();
    let incoming_name = incoming.name.to_lowercase();
    if existing_name != incoming_name {
        return existing_name < incoming_name;
    }

    // 名称相同则保留带图标的候选，保证结果稳定
    existing.icon_path.is_some() || incoming.icon_path.is_none()
}

fn is_usable(candidate: &AppCandidate) -> bool {
    let name = candidate.name.to_lowercase();
    if NON_APP_NAME_HINTS.iter().any(|hint| name.contains(hint)) {
        return false;
    }

    match candidate.kind.as_str() {
        // 仅保留真正的包内应用（AUMID 形如 `<包名>!<入口>`）；Win32 的
        // `AppUserModelID`（如 `Microsoft.Office.WINWORD.EXE.15`）不含 `!`，
        // 已由开始菜单 / App Paths 覆盖，收入会造成同一应用重复出现。
        KIND_UWP => candidate.target.contains('!'),
        _ => {
            let path = Path::new(&candidate.target);
            path.is_file()
                && path
                    .extension()
                    .and_then(|ext| ext.to_str())
                    .map(|ext| {
                        EXECUTABLE_EXTENSIONS
                            .iter()
                            .any(|allowed| ext.eq_ignore_ascii_case(allowed))
                    })
                    .unwrap_or(false)
        }
    }
}

/// 采集全部来源并合并；单项来源失败时降级，不返回错误
pub fn collect() -> AppIndexScan {
    let links = collect_shortcut_paths(&start_menu_dirs());
    let mut stats = ScanStats {
        start_menu_links: links.len(),
        ..ScanStats::default()
    };
    let mut candidates = Vec::new();

    match apps::resolve_shortcuts(&links) {
        Ok(scan) => {
            stats.start_menu_failed = scan.failed;
            for shortcut in scan.resolved {
                candidates.push(start_menu_candidate(shortcut));
            }
        }
        Err(error) => {
            stats.start_menu_failed = links.len();
            log::warn!("开始菜单快捷方式解析整体失败: {}", error);
        }
    }

    let app_paths = apps::list_app_paths();
    stats.app_paths = app_paths.len();
    for entry in app_paths {
        candidates.push(app_path_candidate(entry));
    }

    match apps::list_uwp_apps() {
        Ok(uwp_apps) => {
            stats.uwp = uwp_apps.len();
            for app in uwp_apps {
                candidates.push(uwp_candidate(app));
            }
        }
        Err(error) => {
            stats.uwp_failed = true;
            log::warn!("UWP 枚举失败，跳过该来源: {}", error);
        }
    }

    let rows = build_index(candidates);
    log::info!(
        "app index scan: links={}, failed={}, app_paths={}, uwp={}, uwp_failed={}, rows={}",
        stats.start_menu_links,
        stats.start_menu_failed,
        stats.app_paths,
        stats.uwp,
        stats.uwp_failed,
        rows.len()
    );

    AppIndexScan { rows, stats }
}

/// 开始菜单目录（当前用户 + 全局）
pub fn start_menu_dirs() -> Vec<PathBuf> {
    let mut dirs = Vec::new();
    if let Some(appdata) = std::env::var_os("APPDATA") {
        dirs.push(PathBuf::from(appdata).join("Microsoft\\Windows\\Start Menu\\Programs"));
    }
    if let Some(program_data) = std::env::var_os("ProgramData") {
        dirs.push(PathBuf::from(program_data).join("Microsoft\\Windows\\Start Menu\\Programs"));
    }
    dirs
}

/// 递归收集目录下的 `.lnk`（结果排序去重，便于测试稳定）
pub fn collect_shortcut_paths(dirs: &[PathBuf]) -> Vec<PathBuf> {
    let mut paths = Vec::new();
    for dir in dirs {
        walk_shortcuts(dir, 0, &mut paths);
    }
    paths.sort();
    paths.dedup();
    paths
}

fn walk_shortcuts(dir: &Path, depth: usize, out: &mut Vec<PathBuf>) {
    if depth > MAX_DIR_DEPTH {
        return;
    }
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            walk_shortcuts(&path, depth + 1, out);
        } else if is_shortcut(&path) {
            out.push(path);
        }
    }
}

fn is_shortcut(path: &Path) -> bool {
    path.extension()
        .and_then(|ext| ext.to_str())
        .map(|ext| ext.eq_ignore_ascii_case("lnk"))
        .unwrap_or(false)
}

fn start_menu_candidate(shortcut: ResolvedShortcut) -> AppCandidate {
    let name = shortcut
        .link
        .file_stem()
        .map(|stem| stem.to_string_lossy().to_string())
        .unwrap_or_default();
    let target = shortcut.target.to_string_lossy().to_string();
    let icon_path = shortcut
        .icon_path
        .or_else(|| (!target.is_empty()).then(|| target.clone()));

    AppCandidate {
        name,
        target,
        args: shortcut.args,
        icon_path,
        kind: KIND_WIN32.to_string(),
        source: SOURCE_START_MENU.to_string(),
    }
}

fn app_path_candidate(entry: AppPathEntry) -> AppCandidate {
    let target = entry.target.to_string_lossy().to_string();
    AppCandidate {
        name: entry.name,
        icon_path: (!target.is_empty()).then(|| target.clone()),
        target,
        args: None,
        kind: KIND_WIN32.to_string(),
        source: SOURCE_APP_PATHS.to_string(),
    }
}

fn uwp_candidate(app: UwpApp) -> AppCandidate {
    AppCandidate {
        name: app.name,
        target: app.aumid,
        args: None,
        icon_path: None,
        kind: KIND_UWP.to_string(),
        source: SOURCE_UWP.to_string(),
    }
}

/// 后台刷新周期：ARCHITECTURE §9 规定 v1 用 10 分钟轮询替代 `ReadDirectoryChangesW`
pub const REFRESH_INTERVAL: Duration = Duration::from_secs(600);

/// 停止检查粒度（保证退出时最多等 1s）
const STOP_POLL: Duration = Duration::from_secs(1);

/// 索引重建互斥锁：命令与后台刷新共用，避免并发全量替换
#[derive(Debug, Clone, Default)]
pub struct IndexRebuildLock(Arc<Mutex<()>>);

impl IndexRebuildLock {
    fn guard(&self) -> MutexGuard<'_, ()> {
        // 锁中毒时继续使用内部值：重建是全量替换，事务保证一致性
        self.0
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
    }
}

/// 扫描并写入索引；结果与现有索引一致时跳过写库
pub fn rebuild_index(db_path: &Path, lock: &IndexRebuildLock) -> Result<usize, AppError> {
    let _guard = lock.guard();

    let scan = collect();
    let mut storage = ClipboardStorage::open_write_at(db_path)?;
    let existing = storage.load_app_index()?;

    if rows_equal(&existing, &scan.rows) {
        log::info!("app index unchanged: rows={}", existing.len());
        return Ok(existing.len());
    }

    let written = storage.replace_app_index(&scan.rows, now_ms())?;
    log::info!("app index refreshed: rows={}", written);
    Ok(written)
}

/// 行集合比较：与顺序无关，只比对持久化字段
pub fn rows_equal(left: &[AppIndexRow], right: &[AppIndexRow]) -> bool {
    if left.len() != right.len() {
        return false;
    }
    let mut left_sorted: Vec<&AppIndexRow> = left.iter().collect();
    let mut right_sorted: Vec<&AppIndexRow> = right.iter().collect();
    left_sorted.sort_by(|a, b| a.key.cmp(&b.key));
    right_sorted.sort_by(|a, b| a.key.cmp(&b.key));

    left_sorted.iter().zip(right_sorted).all(|(a, b)| {
        a.key == b.key
            && a.name == b.name
            && a.target == b.target
            && a.args == b.args
            && a.kind == b.kind
            && a.source == b.source
            && a.icon_path == b.icon_path
    })
}

fn now_ms() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|duration| duration.as_millis() as i64)
        .unwrap_or(0)
}

/// 后台刷新线程：启动即重建一次，之后每 10 分钟轮询
pub struct IndexRefresher {
    stop: Arc<AtomicBool>,
    handle: Option<JoinHandle<()>>,
}

impl IndexRefresher {
    /// 启动后台线程（立即返回，不阻塞调用方）
    pub fn start(db_path: PathBuf, lock: IndexRebuildLock) -> Self {
        let stop = Arc::new(AtomicBool::new(false));
        let stop_for_thread = Arc::clone(&stop);
        let handle = std::thread::spawn(move || loop {
            if let Err(error) = rebuild_index(&db_path, &lock) {
                log::warn!("app index refresh failed: {}", error);
            }
            if !wait_for_next(&stop_for_thread) {
                break;
            }
        });

        Self {
            stop,
            handle: Some(handle),
        }
    }
}

impl Drop for IndexRefresher {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Relaxed);
        if let Some(handle) = self.handle.take() {
            let _ = handle.join();
        }
    }
}

/// 分片睡眠：停止标志置位后最多 1s 退出
fn wait_for_next(stop: &AtomicBool) -> bool {
    let mut slept = Duration::ZERO;
    while slept < REFRESH_INTERVAL {
        if stop.load(Ordering::Relaxed) {
            return false;
        }
        std::thread::sleep(STOP_POLL);
        slept += STOP_POLL;
    }
    !stop.load(Ordering::Relaxed)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn existing_exe() -> String {
        match std::env::current_exe() {
            Ok(path) => path.to_string_lossy().to_string(),
            Err(error) => panic!("failed to locate test executable: {}", error),
        }
    }

    fn candidate(name: &str, target: &str, source: &str, kind: &str) -> AppCandidate {
        AppCandidate {
            name: name.to_string(),
            target: target.to_string(),
            args: None,
            icon_path: None,
            kind: kind.to_string(),
            source: source.to_string(),
        }
    }

    #[test]
    #[allow(non_snake_case)]
    fn 同一路径不同来源时保留开始菜单() {
        let exe = existing_exe();
        let rows = build_index(vec![
            candidate(
                "来自 App Paths",
                &exe.to_uppercase(),
                SOURCE_APP_PATHS,
                KIND_WIN32,
            ),
            candidate("来自开始菜单", &exe, SOURCE_START_MENU, KIND_WIN32),
        ]);

        assert_eq!(rows.len(), 1, "同一 exe 只应保留一条");
        assert_eq!(rows[0].name, "来自开始菜单");
        assert_eq!(rows[0].source, SOURCE_START_MENU);
        assert_eq!(rows[0].key, exe.to_lowercase());
    }

    #[test]
    #[allow(non_snake_case)]
    fn 同名同来源时按名称字典序保留() {
        let exe = existing_exe();
        let rows = build_index(vec![
            candidate("zeta", &exe, SOURCE_START_MENU, KIND_WIN32),
            candidate("alpha", &exe, SOURCE_START_MENU, KIND_WIN32),
        ]);

        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].name, "alpha");
    }

    #[test]
    #[allow(non_snake_case)]
    fn 跳过卸载与说明类快捷方式() {
        let exe = existing_exe();
        let rows = build_index(vec![
            candidate("卸载 Foo", &exe, SOURCE_START_MENU, KIND_WIN32),
            candidate("Foo Readme", &exe, SOURCE_START_MENU, KIND_WIN32),
            candidate("Foo", &exe, SOURCE_START_MENU, KIND_WIN32),
        ]);

        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].name, "Foo");
    }

    #[test]
    #[allow(non_snake_case)]
    fn 目标不存在或非可执行文件被跳过() {
        let rows = build_index(vec![
            candidate(
                "缺失",
                "C:\\QuickWin-Not-Exist\\missing.exe",
                SOURCE_START_MENU,
                KIND_WIN32,
            ),
            candidate(
                "文档",
                "C:\\QuickWin-Not-Exist\\readme.txt",
                SOURCE_START_MENU,
                KIND_WIN32,
            ),
        ]);

        assert!(rows.is_empty());
    }

    #[test]
    #[allow(non_snake_case)]
    fn uwp用小写aumid作为键() {
        let rows = build_index(vec![candidate(
            "计算器",
            "Microsoft.WindowsCalculator_8wekyb3d8bbwe!App",
            SOURCE_UWP,
            KIND_UWP,
        )]);

        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].key, "microsoft.windowscalculator_8wekyb3d8bbwe!app");
        assert_eq!(rows[0].kind, KIND_UWP);
    }

    #[test]
    #[allow(non_snake_case)]
    fn 非包内aumid的uwp条目被跳过() {
        let rows = build_index(vec![candidate(
            "Word",
            "Microsoft.Office.WINWORD.EXE.15",
            SOURCE_UWP,
            KIND_UWP,
        )]);

        assert!(rows.is_empty(), "Win32 风格 AUMID 不应重复入索引");
    }

    #[test]
    #[allow(non_snake_case)]
    fn 排序按来源优先级再按名称() {
        let unique = format!(
            "quickwin-appindex-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.as_nanos())
                .unwrap_or(0)
        );
        let dir = std::env::temp_dir().join(unique);
        std::fs::create_dir_all(&dir).expect("create temp dir");

        let alpha = dir.join("alpha.exe");
        let beta = dir.join("beta.exe");
        let zeta = dir.join("zeta.exe");
        for path in [&alpha, &beta, &zeta] {
            std::fs::write(path, b"test").expect("write temp exe");
        }

        let rows = build_index(vec![
            candidate(
                "beta",
                &beta.to_string_lossy(),
                SOURCE_APP_PATHS,
                KIND_WIN32,
            ),
            candidate(
                "zeta",
                &zeta.to_string_lossy(),
                SOURCE_START_MENU,
                KIND_WIN32,
            ),
            candidate(
                "alpha",
                &alpha.to_string_lossy(),
                SOURCE_APP_PATHS,
                KIND_WIN32,
            ),
        ]);

        let names: Vec<&str> = rows.iter().map(|row| row.name.as_str()).collect();
        assert_eq!(names, vec!["zeta", "alpha", "beta"]);

        let _ = std::fs::remove_dir_all(&dir);
    }

    fn app_test_row(key: &str, name: &str) -> AppIndexRow {
        AppIndexRow {
            key: key.to_string(),
            name: name.to_string(),
            target: key.to_string(),
            args: None,
            kind: "win32".to_string(),
            source: "start_menu".to_string(),
            icon_path: None,
        }
    }

    #[test]
    #[allow(non_snake_case)]
    fn 索引比较与顺序无关且能发现变化() {
        let first = app_test_row("a.exe", "Alpha");
        let second = app_test_row("b.exe", "Beta");

        assert!(rows_equal(
            &[first.clone(), second.clone()],
            &[second.clone(), first.clone()]
        ));

        let mut changed = second.clone();
        changed.name = "Beta 2".to_string();
        assert!(!rows_equal(
            &[first.clone(), second.clone()],
            &[first.clone(), changed]
        ));
        assert!(!rows_equal(&[first], &[]));
    }

    /// 真机手测入口：先运行一次应用生成数据库，再执行
    /// `cargo test --lib 真机刷新应用索引两次 -- --ignored --nocapture`
    #[test]
    #[ignore = "仅真机手测运行，会写真实数据库"]
    #[allow(non_snake_case)]
    fn 真机刷新应用索引两次() {
        let Some(data_dir) = dirs::data_dir() else {
            println!("skip: no data dir");
            return;
        };
        let db_path = data_dir.join("com.quickwin.app").join("quickwin.db");
        let lock = IndexRebuildLock::default();

        match rebuild_index(&db_path, &lock) {
            Ok(first) => println!("first rebuild rows={}", first),
            Err(error) => {
                println!("first rebuild failed: {:?}", error);
                return;
            }
        }
        match rebuild_index(&db_path, &lock) {
            Ok(second) => println!("second rebuild rows={}（内容未变时应短路）", second),
            Err(error) => println!("second rebuild failed: {:?}", error),
        }
    }

    /// 真机手测入口：`cargo test --lib 真机扫描应用索引 -- --ignored --nocapture`
    ///
    /// 输出各来源计数、失败数、总耗时与前 50 条名称，用于核对「命中率 ≥90% / ≤2s」。
    #[test]
    #[ignore = "仅真机手测运行"]
    #[allow(non_snake_case)]
    fn 真机扫描应用索引() {
        let started = std::time::Instant::now();
        let scan = collect();
        let elapsed = started.elapsed();

        println!("stats: {:?}", scan.stats);
        println!("rows: {}, elapsed: {:?}", scan.rows.len(), elapsed);
        let mut names: Vec<String> = scan
            .rows
            .iter()
            .map(|row| row.name.to_lowercase())
            .collect();
        names.sort();
        let duplicates: Vec<&String> = names
            .windows(2)
            .filter(|pair| pair[0] == pair[1])
            .map(|pair| &pair[0])
            .collect();
        println!("duplicate names: {:?}", duplicates);
        for row in scan.rows.iter().take(50) {
            println!("{} | {} | {}", row.name, row.source, row.kind);
        }
    }
}
