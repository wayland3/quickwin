/// 日志脱敏工具
///
/// 将文本转换为长度信息，禁止内容入日志。
///
/// # Examples
///
/// ```
/// assert_eq!(redact("hello"), "len=5");
/// assert_eq!(redact(""), "len=0");
/// ```
pub fn redact(text: &str) -> String {
    format!("len={}", text.len())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn 空字符串脱敏() {
        assert_eq!(redact(""), "len=0");
    }

    #[test]
    fn 普通文本脱敏() {
        assert_eq!(redact("hello"), "len=5");
    }

    #[test]
    fn 中文文本脱敏() {
        assert_eq!(redact("你好"), "len=6"); // UTF-8 编码，每个中文字符 3 字节
    }

    #[test]
    fn 长文本脱敏() {
        let long_text = "a".repeat(1000);
        assert_eq!(redact(&long_text), "len=1000");
    }
}
