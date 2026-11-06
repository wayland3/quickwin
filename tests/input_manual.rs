// 输入模拟层手测入口
//
// 运行方式：cargo test --test input_manual -- --ignored
//
// 手测前准备：
// 1. 打开记事本（或其他目标应用）
// 2. 确保焦点在可编辑区域
// 3. 记录环境：Windows 版本、缩放、多屏、输入法状态（中/英）

#[test]
#[ignore]
fn 手测_发送中文英文emoji() {
    // 运行此测试前，请打开记事本并确保焦点在编辑区
    quick::platform::input::send_text("Hello 你好 😀").unwrap();
    // 验证：记事本中显示 "Hello 你好 😀"
}

#[test]
#[ignore]
fn 手测_连续发送100次() {
    // 运行此测试前，请打开记事本并确保焦点在编辑区
    for i in 0..100 {
        quick::platform::input::send_text(&format!("第 {} 次\n", i + 1)).unwrap();
    }
    // 验证：记事本中显示 100 行，无丢失、无重复
}

#[test]
#[ignore]
fn 手测_Ctrl_C() {
    // 运行此测试前，请在记事本中选中文本
    quick::platform::input::send_copy().unwrap();
    // 验证：剪贴板中包含选中文本
}

#[test]
#[ignore]
fn 手测_Ctrl_V() {
    // 运行此测试前，请先复制一些文本到剪贴板
    quick::platform::input::send_paste().unwrap();
    // 验证：记事本中粘贴了剪贴板内容
}

#[test]
#[ignore]
fn 手测_管理员窗口() {
    // 运行此测试前，请以管理员身份打开记事本
    // 预期：返回 AppError::ElevatedTarget
    let result = quick::platform::input::send_text("test");
    assert!(matches!(result, Err(quick::error::AppError::ElevatedTarget)));
}
