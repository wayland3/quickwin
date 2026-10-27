// 剪贴板监听手测入口
//
// 运行方式：cargo test --test clipboard_manual -- --ignored
//
// 手测前准备：
// 1. 记录环境：Windows 版本、缩放、多屏
// 2. 打开目标应用（记事本、Chrome、资源管理器、Excel）
// 3. 确保剪贴板监听已启动

#[test]
#[ignore]
fn 手测_连续复制20次() {
    // 启动监听
    let app = tauri::test::mock_context();
    let mut monitor = quickwin::services::clipboard::ClipboardMonitor::start(app.handle()).unwrap();
    
    // 连续复制 20 次（间隔 < 30ms）
    for i in 0..20 {
        quickwin::platform::input::send_text(&format!("测试 {}", i + 1)).unwrap();
        quickwin::platform::input::send_copy().unwrap();
        std::thread::sleep(std::time::Duration::from_millis(20));
    }
    
    // 验证：全部捕获且无重复事件
    // TODO: 监听 clipboard://changed 事件并计数
    
    monitor.stop();
}

#[test]
#[ignore]
fn 手测_Excel占用剪贴板() {
    // 启动监听
    let app = tauri::test::mock_context();
    let mut monitor = quickwin::services::clipboard::ClipboardMonitor::start(app.handle()).unwrap();
    
    // 打开 Excel 并复制大表格（占用数秒）
    // 手动操作：打开 Excel，选择大表格，Ctrl+C
    
    // 验证：监听线程不卡死，CPU ≈ 0%，最终能捕获
    std::thread::sleep(std::time::Duration::from_secs(10));
    
    monitor.stop();
}

#[test]
#[ignore]
fn 手测_快速复制间隔小于30ms() {
    // 启动监听
    let app = tauri::test::mock_context();
    let mut monitor = quickwin::services::clipboard::ClipboardMonitor::start(app.handle()).unwrap();
    
    // 快速复制 50 次（间隔 10ms）
    for i in 0..50 {
        quickwin::platform::input::send_text(&format!("快速 {}", i + 1)).unwrap();
        quickwin::platform::input::send_copy().unwrap();
        std::thread::sleep(std::time::Duration::from_millis(10));
    }
    
    // 验证：轮询间隔 250ms，部分变化会被合并为一次事件
    // 这是预期行为，记录捕获的事件数量
    
    monitor.stop();
}

#[test]
#[ignore]
fn 手测_线程优雅关闭() {
    // 启动监听
    let app = tauri::test::mock_context();
    let monitor = quickwin::services::clipboard::ClipboardMonitor::start(app.handle()).unwrap();
    
    // 等待一段时间
    std::thread::sleep(std::time::Duration::from_secs(2));
    
    // Drop 时自动停止
    drop(monitor);
    
    // 验证：线程已退出，无僵尸进程
}
