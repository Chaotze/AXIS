// ============================================================
// 哈希函数库
// ============================================================
// 提供 FNV-1a 与 DJB2 两种非密码学哈希函数。
//
// 为什么选择 FNV-1a：
// - 实现极简：每个字节一次异或 + 一次乘法，无分支、无查表，
//   适合内核这种资源受限环境
// - 分布均匀性对短键（整数、短字符串）足够好，
//   足以支撑内核哈希表（如无锁 hashmap、名称散列）的需求
// - 不需要随机种子：内核启动早期没有可靠的熵源，
//   带密钥的 SipHash 需要管理密钥生命周期，复杂度收益不成比例
//
// 为什么不选 CRC 充当哈希：
// - CRC 的数学结构（线性）使攻击者可以轻易构造碰撞输入，
//   且查表法引入缓存开销；CRC 的定位是"检错"而非"散列"
//
// 为什么保留 DJB2：
// - 实现同样极简（移位加法代替乘法），部分场景（如短符号名）
//   是业界习惯用法，提供多一种无状态选择

/// FNV-1a 64 位哈希
///
/// 参数为 FNV 标准常量：偏移基 0xcbf29ce484222325、
/// 素数 0x100000001b3。
#[inline]
pub fn fnv1a64(bytes: &[u8]) -> u64 {
    const OFFSET_BASIS: u64 = 0xcbf2_9ce4_8422_2325;
    const PRIME: u64 = 0x0000_0100_0000_01b3;

    let mut hash = OFFSET_BASIS;
    for &byte in bytes {
        hash ^= byte as u64;
        hash = hash.wrapping_mul(PRIME);
    }
    hash
}

/// FNV-1a 32 位哈希
#[inline]
pub fn fnv1a32(bytes: &[u8]) -> u32 {
    const OFFSET_BASIS: u32 = 0x811c_9dc5;
    const PRIME: u32 = 0x0100_0193;

    let mut hash = OFFSET_BASIS;
    for &byte in bytes {
        hash ^= byte as u32;
        hash = hash.wrapping_mul(PRIME);
    }
    hash
}

/// DJB2 64 位哈希
///
/// 递推式：hash = hash * 33 + c；`* 33` 用移位加法实现
/// （`(hash << 5) + hash`），避免乘法器的使用。
#[inline]
pub fn djb2_64(bytes: &[u8]) -> u64 {
    let mut hash: u64 = 5381; // DJB2 的初始魔数
    for &byte in bytes {
        hash = ((hash << 5).wrapping_add(hash)).wrapping_add(byte as u64);
    }
    hash
}

/// DJB2 32 位哈希
#[inline]
pub fn djb2_32(bytes: &[u8]) -> u32 {
    let mut hash: u32 = 5381;
    for &byte in bytes {
        hash = ((hash << 5).wrapping_add(hash)).wrapping_add(byte as u32);
    }
    hash
}

/// 对 u64 键做哈希
///
/// 为什么单独提供整型键入口：
/// - 内核哈希表（锁、ID 映射等）的键绝大多数是 u64/usize，
///   让使用方自行转字节数组会造成重复代码
/// - 集中在这里保证所有整型键使用同一哈希算法，
///   调用方只依赖"hash_u64"这一语义
#[inline]
pub fn hash_u64(value: u64) -> u64 {
    fnv1a64(&value.to_le_bytes())
}

/// 对 usize 键做哈希（返回 usize，直接可用于取模）
#[inline]
pub fn hash_usize(value: usize) -> usize {
    fnv1a64(&value.to_le_bytes()) as usize
}

#[cfg(test)]
mod tests {
    extern crate std;

    use super::*;

    #[test]
    fn test_fnv1a64_known_vectors() {
        // FNV-1a 64 位官方测试向量
        assert_eq!(fnv1a64(b""), 0xcbf2_9ce4_8422_2325);
        assert_eq!(fnv1a64(b"a"), 0xaf63_dc4c_8601_ec8c);
        assert_eq!(fnv1a64(b"foobar"), 0x8594_4171_f739_67e8);
    }

    #[test]
    fn test_fnv1a32_known_vectors() {
        // FNV-1a 32 位官方测试向量
        assert_eq!(fnv1a32(b""), 0x811c_9dc5);
        assert_eq!(fnv1a32(b"a"), 0xe40c_292c);
    }

    #[test]
    fn test_djb2_known_vectors() {
        assert_eq!(djb2_64(b""), 5381);
        // "a" = 5381 * 33 + 97
        assert_eq!(djb2_64(b"a"), 177_670);
        assert_eq!(djb2_32(b""), 5381);
    }

    #[test]
    fn test_hash_u64_stable() {
        // 同一输入必须产出同一哈希（确定性）
        assert_eq!(hash_u64(42), hash_u64(42));
        // 哈希应随输入变化（基本雪崩性检查：不同键不同值）
        assert_ne!(hash_u64(0), hash_u64(1));
        // usize 入口与 u64 入口对小值一致（低位截断）
        assert_eq!(hash_usize(7) as u64, hash_u64(7) & (usize::MAX as u64));
    }
}
