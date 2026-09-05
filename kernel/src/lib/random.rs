// ============================================================
// 随机数生成器 - 64 位梅森旋转算法（MT19937-64）
// ============================================================
// 提供高质量伪随机数生成能力
//
// 为什么使用 MT19937-64：
// - 周期长达 2^19937-1，足以覆盖所有应用场景
// - 统计特性优秀，通过 Diehard 随机性测试
// - 在内核环境中被广泛采用
// - 无需额外硬件熵源（可配合系统启动时间作为种子）

const NN: usize = 312;
const MM: usize = 156;
const MATRIX_A: u64 = 0xb5026f55u64;
const UM: u64 = 0xffffffff80000000u64;
const LM: u64 = 0x7fffffffu64;

/// 64 位梅森旋转随机数生成器
///
/// 为什么使用静态全局状态而非栈变量：
/// - 内核中的随机数生成需要全局、持久的状态
/// - 多次调用间保持连续性，提高随机性质量
/// - 避免重复初始化的开销
pub struct Mt19937 {
    /// MT 数组
    mt: [u64; NN],
    /// 当前指针
    mti: usize,
}

/// 全局随机数生成器实例
impl Mt19937 {
    /// 创建新的随机数生成器
    pub const fn new() -> Self {
        Mt19937 {
            mt: [0u64; NN],
            mti: NN + 1,
        }
    }

    /// 使用种子初始化生成器
    pub fn init_genrand(&mut self, seed: u64) {
        self.mt[0] = seed;
        for i in 1..NN {
            self.mt[i] = (6364136223846793005u64)
                .wrapping_mul(self.mt[i - 1] ^ (self.mt[i - 1] >> 62))
                .wrapping_add(i as u64);
        }
        self.mti = NN;
    }

    /// 初始化扭转生成器（当状态耗尽时调用）
    fn twist(&mut self) {
        let mag01 = [0u64, MATRIX_A];

        for kk in 0..NN - MM {
            let y = (self.mt[kk] & UM) | (self.mt[kk + 1] & LM);
            self.mt[kk] = self.mt[kk + MM] ^ (y >> 1) ^ mag01[(y & 1u64) as usize];
        }

        for kk in (NN - MM)..NN - 1 {
            let y = (self.mt[kk] & UM) | (self.mt[kk + 1] & LM);
            // 修正：应该是 kk + (MM - NN) 对应于负的偏移
            // 等价于 kk - (NN - MM)，也就是绕回到前面
            self.mt[kk] = self.mt[kk - (NN - MM)] ^ (y >> 1) ^ mag01[(y & 1u64) as usize];
        }

        let y = (self.mt[NN - 1] & UM) | (self.mt[0] & LM);
        self.mt[NN - 1] = self.mt[MM - 1] ^ (y >> 1) ^ mag01[(y & 1u64) as usize];

        self.mti = 0;
    }

    /// 生成下一个随机数
    pub fn genrand_int64(&mut self) -> u64 {
        if self.mti >= NN {
            if self.mti > NN {
                // 生成器未初始化，使用默认种子
                self.init_genrand(5489u64);
            }
            self.twist();
        }

        let mut y = self.mt[self.mti];
        self.mti += 1;

        // 模板变换
        y ^= (y >> 29) & 0x5555555555555555u64;
        y ^= (y << 17) & 0x71d67fffeda60000u64;
        y ^= (y << 37) & 0xfff7eee000000000u64;
        y ^= y >> 43;

        y
    }
}

/// 全局随机数生成器实例
static GLOBAL_RNG: Spinlock<Mt19937> = Spinlock::new(Mt19937::new());

use crate::sync::Spinlock;

/// 初始化全局随机数生成器
///
/// 使用系统启动时间作为初始种子
pub fn init_random() {
    let seed = crate::lib::time::uptime_ms() as u64;
    let mut rng = GLOBAL_RNG.lock();
    rng.init_genrand(seed);
}

/// 获取下一个 64 位随机数
#[inline]
pub fn random_u64() -> u64 {
    let mut rng = GLOBAL_RNG.lock();
    rng.genrand_int64()
}

/// 获取 0 到 max 之间的随机数（包含 0，不包含 max）
#[inline]
pub fn random_range(max: u64) -> u64 {
    if max == 0 {
        return 0;
    }
    random_u64() % max
}

/// 获取下一个随机字节
#[inline]
pub fn random_u8() -> u8 {
    (random_u64() >> 24) as u8
}

#[cfg(test)]
mod tests {
    extern crate std;
    use super::*;

    #[test]
    fn test_mt19937_deterministic() {
        // 相同的种子应该产生相同的序列
        let mut rng1 = Mt19937::new();
        rng1.init_genrand(12345);

        let mut rng2 = Mt19937::new();
        rng2.init_genrand(12345);

        for _ in 0..100 {
            assert_eq!(rng1.genrand_int64(), rng2.genrand_int64());
        }
    }

    #[test]
    fn test_mt19937_output_range() {
        let mut rng = Mt19937::new();
        rng.init_genrand(54321);

        // 生成一些随机数，检查范围
        for _ in 0..1000 {
            let _val = rng.genrand_int64();
            // 任何 u64 都是有效的
        }
    }
}
