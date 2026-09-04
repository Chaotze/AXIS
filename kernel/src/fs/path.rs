// ============================================================
// 路径解析模块
// ============================================================
// 提供路径解析、验证和规范化的纯逻辑函数。
// 本模块无全局状态、无锁、无 arch 依赖，可被 unitest 宿主测试。

use crate::lib::result::{KernelError, KernelResult};

// ============================================================
// 路径解析结果
// ============================================================

/// 解析后的路径组件
#[derive(Debug, Clone)]
pub struct PathComponent<'a> {
    /// 路径组件（字节切片）
    pub component: &'a [u8],
    /// 是否为根路径
    pub is_root: bool,
    /// 是否为当前目录（.）
    pub is_current: bool,
    /// 是否为父目录（..）
    pub is_parent: bool,
    /// 是否为绝对路径
    pub is_absolute: bool,
}

impl<'a> PathComponent<'a> {
    /// 获取组件的字符串表示（假设 UTF-8）
    pub fn as_str(&self) -> Option<&'a str> {
        core::str::from_utf8(self.component).ok()
    }
}

/// 路径分解结果（已规范化）
#[derive(Debug, Clone)]
pub struct ParsedPath<'a> {
    /// 原始路径
    pub raw: &'a [u8],
    /// 是否为绝对路径
    pub is_absolute: bool,
    /// 路径组件（不包括根 / 和 .）
    pub components: alloc::vec::Vec<&'a [u8]>,
}

// ============================================================
// 路径验证和规范化
// ============================================================

/// 检查路径是否有效
/// 规则：
/// - 不能为空
/// - 长度不超过 4096 字节（PATH_MAX）
/// - 单个组件不超过 255 字节（NAME_MAX）
pub fn validate_path(path: &[u8]) -> KernelResult<()> {
    // 为什么检查空路径：任何文件系统操作都需要合法的路径
    if path.is_empty() {
        return Err(KernelError::InvalidArgument);
    }

    // 为什么限制路径长度：防止栈溢出和内存浪费
    const PATH_MAX: usize = 4096;
    if path.len() > PATH_MAX {
        return Err(KernelError::InvalidArgument);
    }

    // 为什么检查组件长度：POSIX 标准 NAME_MAX = 255
    const NAME_MAX: usize = 255;
    for component in path.split(|&b| b == b'/') {
        if component.len() > NAME_MAX {
            return Err(KernelError::InvalidArgument);
        }
    }

    Ok(())
}

/// 解析路径为组件序列
/// 处理规则：
/// - 去掉多个连续的 /
/// - 把 . 解析为"不动作"
/// - 把 .. 记录为"上升一级"
/// 返回规范化后的组件向量
pub fn parse_path(path: &[u8]) -> KernelResult<ParsedPath<'_>> {
    validate_path(path)?;

    let is_absolute = !path.is_empty() && path[0] == b'/';
    let mut components = alloc::vec::Vec::new();

    // 为什么分割路径：避免一次性处理整个字符串
    for component in path.split(|&b| b == b'/') {
        // 为什么过滤空组件：连续的 / 会产生空字符串
        if component.is_empty() {
            continue;
        }

        // 为什么处理 .：. 表示当前目录，不应影响路径
        if component == b"." {
            continue;
        }

        // 为什么处理 ..：.. 表示父目录，需要特殊处理
        if component == b".." {
            // 为什么 pop：.. 抵消上一级的前进
            // 但不能超过根目录（绝对路径时）
            if !is_absolute || !components.is_empty() {
                components.pop();
            }
            continue;
        }

        // 普通组件直接加入
        components.push(component);
    }

    Ok(ParsedPath {
        raw: path,
        is_absolute,
        components,
    })
}

/// 规范化路径（移除 . / .. 等多余成分）
pub fn normalize_path(path: &[u8]) -> KernelResult<alloc::vec::Vec<u8>> {
    let parsed = parse_path(path)?;

    let mut result = alloc::vec::Vec::new();

    // 为什么重建路径：确保规范化形式
    if parsed.is_absolute {
        result.push(b'/');
    }

    for (idx, component) in parsed.components.iter().enumerate() {
        if idx > 0 {
            result.push(b'/');
        }
        result.extend_from_slice(component);
    }

    // 为什么检查结果为空：绝对路径至少要有根 /
    if result.is_empty() && parsed.is_absolute {
        result.push(b'/');
    }

    Ok(result)
}

/// 获取文件名（路径的最后一个组件）
pub fn basename(path: &[u8]) -> Option<&[u8]> {
    // 为什么从后向前查找：/a/b/c 中 basename 是 c
    for i in (0..path.len()).rev() {
        if path[i] == b'/' {
            if i + 1 < path.len() {
                return Some(&path[i + 1..]);
            }
        }
    }

    // 为什么检查根目录：/ 的 basename 是空
    if path.is_empty() {
        return None;
    }

    Some(path)
}

/// 获取父目录路径（路径的目录部分）
pub fn dirname(path: &[u8]) -> &[u8] {
    // 为什么从后向前查找：找到最后一个 / 的位置
    for i in (0..path.len()).rev() {
        if path[i] == b'/' {
            if i == 0 {
                // 为什么返回 /：/a 的父目录是 /
                return b"/";
            }
            return &path[..i];
        }
    }

    // 为什么返回 .：a/b 的父目录是 . 或 a
    // 此处假设相对路径
    b"."
}

/// 检查路径是否为相对路径
pub fn is_relative(path: &[u8]) -> bool {
    path.is_empty() || path[0] != b'/'
}

/// 检查路径是否为绝对路径
pub fn is_absolute(path: &[u8]) -> bool {
    !path.is_empty() && path[0] == b'/'
}

/// 检查是否存在符号链接循环风险（通过深度限制）
/// 为什么这样做：符号链接可能形成循环（如 a → b, b → a）
/// 解决方案：限制符号链接展开的最大深度（通常 40）
pub const MAX_SYMLINK_DEPTH: usize = 40;

/// 检查给定的深度是否超过符号链接循环限制
pub fn check_symlink_depth(depth: usize) -> KernelResult<()> {
    if depth > MAX_SYMLINK_DEPTH {
        // 为什么返回此错误：表示符号链接过多/循环
        Err(KernelError::InvalidArgument)  // 应该有 SymlinkLoop 错误
    } else {
        Ok(())
    }
}

// ============================================================
// 单元测试（逻辑验证）
// ============================================================

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_validate_path() {
        assert!(validate_path(b"/").is_ok());
        assert!(validate_path(b"/a/b/c").is_ok());
        assert!(validate_path(b"").is_err());  // 空路径
    }

    #[test]
    fn test_parse_path() {
        let parsed = parse_path(b"/a/b/c").unwrap();
        assert!(parsed.is_absolute);
        assert_eq!(parsed.components.len(), 3);
        assert_eq!(parsed.components[0], b"a");
        assert_eq!(parsed.components[1], b"b");
        assert_eq!(parsed.components[2], b"c");
    }

    #[test]
    fn test_normalize_dots() {
        // /a/./b → /a/b
        let result = normalize_path(b"/a/./b").unwrap();
        assert_eq!(result, b"/a/b");

        // /a/../b → /b
        let result = normalize_path(b"/a/../b").unwrap();
        assert_eq!(result, b"/b");
    }

    #[test]
    fn test_basename() {
        assert_eq!(basename(b"/a/b/c"), Some(b"c".as_ref()));
        assert_eq!(basename(b"/a"), Some(b"a".as_ref()));
        assert_eq!(basename(b"/"), Some(b"/".as_ref()));
    }

    #[test]
    fn test_dirname() {
        assert_eq!(dirname(b"/a/b/c"), b"/a/b");
        assert_eq!(dirname(b"/a"), b"/");
        assert_eq!(dirname(b"a/b"), b"a");
    }
}
