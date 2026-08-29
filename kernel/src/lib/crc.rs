// ============================================================
// CRC 校验码计算
// ============================================================
// 提供 CRC32（IEEE 802.3）与 CRC16（CCITT）校验。
//
// 为什么内核需要 CRC：
// - 网络协议栈（以太网帧尾的 FCS 字段）、文件系统元数据、
//   内核模块校验（loadable.md 中的符号 CRC）都依赖 CRC
//   来低成本地发现随机错误
// - 与哈希不同，CRC 的数学性质保证"任意 1~3 位翻转必检出"，
//   这是检错场景选它的原因
//
// 为什么 CRC32 用查表法：
// - 逐位计算每字节需要 8 次循环迭代，查表法一次完成，
//   以 1KB 只读静态表换取约 8 倍吞吐
// - 表在编译期由 const fn 生成：裸机环境没有运行时初始化
//   执行器（.init_array），运行期建表反而不可行
//
// 为什么 CRC16 保留逐位实现：
// - CCITT 多项式主要用于小型控制报文，吞吐不敏感；
//   逐位实现零表空间，与查表实现形成"时空权衡"的对照

/// 编译期生成 CRC32（IEEE 802.3，反射多项式 0xEDB88320）查表
///
/// # 为什么必须是 const fn：
/// - 生成的表被静态常量引用；const fn 保证表在编译期
///   完全确定，二进制中直接是 .rodata，运行期零成本
const fn make_crc32_table() -> [u32; 256] {
    let mut table = [0u32; 256];
    let mut i = 0;
    while i < 256 {
        let mut crc = i as u32;
        let mut bit = 0;
        while bit < 8 {
            // 反射算法：低位先处理；crc 为 1 时右移并异或多项式
            crc = if crc & 1 != 0 {
                (crc >> 1) ^ 0xEDB8_8320
            } else {
                crc >> 1
            };
            bit += 1;
        }
        table[i] = crc;
        i += 1;
    }
    table
}

/// CRC32 查表（编译期常量，位于 .rodata，无初始化开销）
static CRC32_TABLE: [u32; 256] = make_crc32_table();

/// 计算 CRC32（IEEE 802.3，即常见 zip/以太网使用的变体）
///
/// 初始值 0xFFFF_FFFF，结果按位取反输出（xorout），
/// 校验值 "123456789" → 0xCBF43926。
#[inline]
pub fn crc32(data: &[u8]) -> u32 {
    let mut crc: u32 = 0xFFFF_FFFF;
    for &byte in data {
        let index = ((crc ^ byte as u32) & 0xFF) as usize;
        crc = (crc >> 8) ^ CRC32_TABLE[index];
    }
    !crc
}

/// 计算 CRC16-CCITT（多项式 0x1021，初值 0xFFFF，无反射）
///
/// 校验值 "123456789" → 0x29B1。逐位实现：
/// 每次处理一个位，最高位为 1 时左移并异或多项式。
pub fn crc16_ccitt(data: &[u8]) -> u16 {
    let mut crc: u16 = 0xFFFF;
    for &byte in data {
        crc ^= (byte as u16) << 8;
        for _ in 0..8 {
            crc = if crc & 0x8000 != 0 {
                (crc << 1) ^ 0x1021
            } else {
                crc << 1
            };
        }
    }
    crc
}

#[cfg(test)]
mod tests {
    extern crate std;

    use super::*;

    #[test]
    fn test_crc32_known_vector() {
        // IEEE 802.3 标准校验值
        assert_eq!(crc32(b"123456789"), 0xCBF4_3926);
        // 空输入：初值取反 = 0
        assert_eq!(crc32(b""), 0);
    }

    #[test]
    fn test_crc32_detects_single_bit_flip() {
        let original = crc32(b"kernel data block");
        // 翻转中间一个字节的一个位，校验值必须变化
        let mut corrupted = b"kernel data block".to_vec();
        corrupted[7] ^= 0x01;
        assert_ne!(original, crc32(&corrupted));
    }

    #[test]
    fn test_crc16_ccitt_known_vector() {
        // CRC-16/CCITT-FALSE 标准校验值
        assert_eq!(crc16_ccitt(b"123456789"), 0x29B1);
    }

    #[test]
    fn test_crc_table_consistency() {
        // 表由 const fn 生成，抽样与标准 CRC32 查表值核对
        assert_eq!(CRC32_TABLE[0], 0);
        assert_eq!(CRC32_TABLE[1], 0x7707_3096);
        assert_eq!(CRC32_TABLE[2], 0xEE0E_612C);
    }
}
