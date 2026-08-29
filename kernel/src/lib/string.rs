// ============================================================
// 字符串处理
// ============================================================
// 提供 no_std 环境下的字符串操作

/// 计算 C 字符串长度
///
/// 从指针开始查找，直到遇到 '\0'
///
/// # Safety
/// 调用者必须确保指针有效且字符串以 '\0' 结尾
#[inline]
pub unsafe fn strlen(s: *const u8) -> usize {
    let mut len = 0;
    unsafe {
        while *s.add(len) != 0 {
            len += 1;
        }
    }
    len
}

/// 比较两个字节序列
#[inline]
pub fn memcmp(a: &[u8], b: &[u8]) -> core::cmp::Ordering {
    use core::cmp::Ordering;

    let len = a.len().min(b.len());
    for i in 0..len {
        match a[i].cmp(&b[i]) {
            Ordering::Equal => continue,
            other => return other,
        }
    }
    a.len().cmp(&b.len())
}

/// 复制内存
///
/// # Safety
/// 调用者必须确保指针有效且不重叠
#[inline]
pub unsafe fn memcpy(dst: *mut u8, src: *const u8, count: usize) {
    unsafe {
        core::ptr::copy_nonoverlapping(src, dst, count);
    }
}

/// 设置内存
///
/// # Safety
/// 调用者必须确保指针有效
#[inline]
pub unsafe fn memset(dst: *mut u8, value: u8, count: usize) {
    unsafe {
        core::ptr::write_bytes(dst, value, count);
    }
}

/// 移动内存（允许重叠）
///
/// # Safety
/// 调用者必须确保指针有效
#[inline]
pub unsafe fn memmove(dst: *mut u8, src: *const u8, count: usize) {
    unsafe {
        core::ptr::copy(src, dst, count);
    }
}

/// 判断字节序列是否以指定前缀开头
///
/// 供路径解析、协议解析等"先看头部再决定"的场景使用，
/// 避免调用方各自手写比较循环。
#[inline]
pub fn starts_with(haystack: &[u8], needle: &[u8]) -> bool {
    haystack.len() >= needle.len() && memcmp(&haystack[..needle.len()], needle).is_eq()
}

/// 判断字节序列是否以指定后缀结尾
#[inline]
pub fn ends_with(haystack: &[u8], needle: &[u8]) -> bool {
    haystack.len() >= needle.len() && memcmp(&haystack[haystack.len() - needle.len()..], needle).is_eq()
}

/// 查找字节在序列中首次出现的位置
#[inline]
pub fn find_byte(haystack: &[u8], byte: u8) -> Option<usize> {
    haystack.iter().position(|&b| b == byte)
}

/// 查找字节在序列中最后一次出现的位置
#[inline]
pub fn rfind_byte(haystack: &[u8], byte: u8) -> Option<usize> {
    haystack.iter().rposition(|&b| b == byte)
}

#[cfg(test)]
mod tests {
    extern crate std;

    use super::*;

    #[test]
    fn test_strlen() {
        let s = b"hello\0world";
        assert_eq!(unsafe { strlen(s.as_ptr()) }, 5);
        assert_eq!(unsafe { strlen(b"\0".as_ptr()) }, 0);
    }

    #[test]
    fn test_memcmp_ordering() {
        use core::cmp::Ordering;
        assert_eq!(memcmp(b"abc", b"abc"), Ordering::Equal);
        assert_eq!(memcmp(b"abc", b"abd"), Ordering::Less);
        assert_eq!(memcmp(b"abd", b"abc"), Ordering::Greater);
        // 前缀关系：短者为小
        assert_eq!(memcmp(b"ab", b"abc"), Ordering::Less);
    }

    #[test]
    fn test_memcpy_memset() {
        let mut dst = [0u8; 8];
        let src = [1u8, 2, 3, 4, 5, 6, 7, 8];
        unsafe { memcpy(dst.as_mut_ptr(), src.as_ptr(), 8) };
        assert_eq!(dst, src);

        unsafe { memset(dst.as_mut_ptr(), 0xFF, 4) };
        assert_eq!(dst, [0xFF, 0xFF, 0xFF, 0xFF, 5, 6, 7, 8]);
    }

    #[test]
    fn test_starts_ends_find() {
        assert!(starts_with(b"/usr/bin", b"/usr"));
        assert!(!starts_with(b"/usr", b"/usr/bin"));
        assert!(ends_with(b"file.elf", b".elf"));
        assert!(!ends_with(b"file.elf", b".exe"));

        assert_eq!(find_byte(b"/usr/bin", b'/'), Some(0));
        assert_eq!(find_byte(b"/usr/bin", b'z'), None);
        assert_eq!(rfind_byte(b"/usr/bin", b'/'), Some(4));
    }
}
