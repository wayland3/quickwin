// 启动器执行手测入口
//
// 运行方式：cargo test --test launcher_manual -- --ignored --nocapture
//
// 这些用例会真的拉起应用，只能在本机手工执行。

#[test]
#[ignore]
fn 手测_启动记事本() {
    quickwin::platform::apps::launch_win32(r"C:\Windows\System32\notepad.exe", None)
        .expect("launch notepad");
    // 验证：记事本窗口在前台打开
}

#[test]
#[ignore]
fn 手测_启动UWP计算器() {
    quickwin::platform::apps::launch_uwp("Microsoft.WindowsCalculator_8wekyb3d8bbwe!App")
        .expect("launch calculator");
    // 验证：计算器窗口打开；若本机没有该 AUMID，从 Get-StartApps 取本机的值再改
}
