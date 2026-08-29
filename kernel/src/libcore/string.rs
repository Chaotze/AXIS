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
