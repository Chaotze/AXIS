// ============================================================
// 内存管理（Memory Management）根模块
// ============================================================
// 聚合三大子系统：
//   物理内存（pmm）→ 虚拟内存（vmm）→ 内核堆（heap）
// 并对外提供：
//   - 统一初始化入口 init()（按依赖顺序 pmm → heap → vmm）
//   - 内存统计与监控接口 stats()
//   - 内核启动自测与压力测试 selftest()（验收标准的内核内验证）
//
// 依赖关系（单向）：
//   pmm（页/区域/伙伴）← heap（SLUB 从 pmm 取页）
//   pmm / heap ← vmm（映射器分配页表页与匿名页）
// 锁序约定：VMM → PMM → HEAP。

pub mod addr;
pub mod heap;
pub mod pmm;
pub mod vmm;

use crate::prelude::KernelResult;

/// 内存管理初始化：pmm → heap → vmm
///
/// 顺序为什么如此：
/// 1. PMM 先接管物理页（堆与 VMM 都依赖它取页）
/// 2. 堆随后就绪（VMM 的 VMA 管理需要 Vec 等堆容器）
/// 3. VMM 最后装配（基于当前页表 + 堆容器）
pub fn init() -> KernelResult<()> {
    // 1. 物理内存管理
    pmm::init()?;
    // 2. 堆分配器
    heap::init();
    // 3. 虚拟内存管理
    vmm::init()?;
    Ok(())
}

/// 汇总内存统计（监控接口的顶层形态）
#[derive(Debug, Clone, Copy, Default)]
pub struct MemoryStats {
    /// 物理页总量
    pub total_pages: usize,
    /// 物理页空闲数
    pub free_pages: usize,
    /// 物理页已用数
    pub used_pages: usize,
    /// 堆对象数
    pub heap_objects: usize,
    /// 堆估算占用字节
    pub heap_bytes: usize,
    /// 缺页总数
    pub page_faults: u64,
    /// 按需分页成功数
    pub demand_pages: u64,
    /// COW 拆解数
    pub cow_breaks: u64,
    /// 换出登记页数
    pub swapped_pages: usize,
}

/// 获取汇总统计（监控接口）
pub fn stats() -> MemoryStats {
    let p = pmm::stats();
    let h = heap::stats();
    let v = vmm::stats();
    MemoryStats {
        total_pages: p.total_pages,
        free_pages: p.free_pages,
        used_pages: p.used_pages,
        heap_objects: h.objects,
        heap_bytes: h.bytes_in_use,
        page_faults: v.page_faults,
        demand_pages: v.demand_pages,
        cow_breaks: v.cow_breaks,
        swapped_pages: v.swapped_pages,
    }
}

/// 打印内存统计（监控输出）
pub fn print_stats() {
    let s = stats();
    println!("[MEM] total={}MB free={}MB used={}MB heap_objs={} heap_bytes={}B",
        s.total_pages / 256, s.free_pages / 256, s.used_pages / 256,
        s.heap_objects, s.heap_bytes);
    println!("[MEM] pf={} demand={} cow={} swapped={}",
        s.page_faults, s.demand_pages, s.cow_breaks, s.swapped_pages);
}

// ---------------------------------------------------------------------
// 内核启动自测（验收标准的内核内验证）
// ---------------------------------------------------------------------

/// 运行全部内存管理自测。
/// 每个子项独立报 PASS/FAIL；返回全部是否通过。
pub fn selftest() -> bool {
    println!("\n[MEM-SELFTEST] === Memory Management Selftest ===");
    let mut all = true;
    all &= t("buddy alloc/free/merge", selftest_buddy());
    all &= t("heap kmalloc/kfree stress", selftest_heap());
    all &= t("Box/Vec/String containers", selftest_containers());
    all &= t("VMA manager", selftest_vma());
    all &= t("demand paging", selftest_demand_paging());
    all &= t("copy-on-write (COW)", selftest_cow());
    all &= t("swap out/in", selftest_swap());
    println!("[MEM-SELFTEST] === Result: {} ===", if all { "ALL PASS" } else { "FAILED" });
    all
}

/// 单测断言容器：记录并打印结果
fn t(name: &str, ok: bool) -> bool {
    if ok {
        println!("  [PASS] {}", name);
    } else {
        println!("  [FAIL] {}", name);
    }
    ok
}

/// 验收用宏：条件不满足返回 false（可带失败说明）
macro_rules! check {
    ($cond:expr $(, $msg:expr)?) => {
        if !($cond) {
            $(println!("    [check] FAILED at: {}", $msg);)?
            return false;
        }
    };
}

/// 1) 伙伴系统：分配/释放/合并的正确性（经由 PMM 接口）
fn selftest_buddy() -> bool {
    use crate::mm::pmm::frame::FrameOwner;
    use crate::mm::pmm::{free_pages, GfpFlags};
    let before = pmm::stats().free_pages;
    // 分配一批不同阶的块
    let mut blocks: alloc::vec::Vec<(crate::arch::x86_64::memory::PhysAddr, usize)> = alloc::vec::Vec::new();
    for order in 0..4 {
        let phys = pmm::alloc_pages(order, GfpFlags::NONE, FrameOwner::Kernel);
        check!(phys.is_some());
        blocks.push((phys.unwrap(), order));
    }
    // 全部释放；随后立即 drop blocks —— Vec 自身在堆上的缓冲若不释放，
    // 会继续占用物理页，导致“页数恢复”的比较产生假阴性
    for (phys, order) in blocks.drain(..) {
        free_pages(phys, order);
    }
    drop(blocks);
    let after = pmm::stats().free_pages;
    check!(after == before, "释放后页数应恢复");
    true
}

/// 2) 堆分配压力：通过全局分配器大量分配/释放，验证桶稳定与页回收
fn selftest_heap() -> bool {
    let h0 = heap::stats();
    let p0 = pmm::stats().free_pages;

    for round in 0..200u32 {
        let mut vec: alloc::vec::Vec<u64> = alloc::vec::Vec::new();
        for i in 0..64u64 {
            vec.push(round as u64 * 1000 + i);
        }
        check!(vec.iter().sum::<u64>() == (0..64).map(|i| round as u64 * 1000 + i).sum());
        // vec 离开作用域自动释放
    }

    let h1 = heap::stats();
    check!(h1.objects <= h0.objects, "压力后堆对象数应不增（无泄漏）");
    let p1 = pmm::stats().free_pages;
    // 允许少量页滞留（某些缓存页在维护中），但应基本恢复
    check!(p1 + 8 >= p0, "压力后物理页应基本归还");
    true
}

/// 3) 标准容器：Box / Vec / String 在全局分配器上的可用性
fn selftest_containers() -> bool {
    let boxed = alloc::boxed::Box::new(42u64);
    check!(*boxed == 42);
    drop(boxed);

    let mut s = alloc::string::String::new();
    for _ in 0..100 {
        s.push('x');
    }
    check!(s.len() == 100);
    drop(s);

    let mut v: alloc::vec::Vec<u8> = alloc::vec::Vec::with_capacity(1024);
    for i in 0..1024u16 {
        v.push((i % 255) as u8);
    }
    check!(v.len() == 1024);
    check!(v.iter().map(|&x| x as u16).sum::<u16>() == (0..1024u16).map(|i| (i % 255) as u16).sum());
    true
}

/// 4) VMA：mmap / mprotect / munmap / 查找
fn selftest_vma() -> bool {
    use crate::mm::vmm::vma::{VmaFlags, VmaPerm};
    let base = crate::config::MM_SELFTEST_BASE as usize;
    let hint = base;

    let reg = vmm::mmap_anon(Some(hint), 0x4000, VmaPerm::read_write(), VmaFlags { anonymous: true, ..VmaFlags::empty() });
    check!(reg.is_ok());
    let addr = reg.unwrap();
    check!(vmm::find_vma(addr).is_some());

    // mprotect 只读
    check!(vmm::mprotect(addr, 0x4000, VmaPerm::read()).is_ok());
    let v = vmm::find_vma(addr).unwrap();
    check!(!v.perms.write);

    // munmap
    check!(vmm::munmap(addr, 0x4000).is_ok());
    check!(vmm::find_vma(addr).is_none());
    true
}

/// 5) 按需分页：mmap 后不分配，首访触发缺页由内核解析
fn selftest_demand_paging() -> bool {
    use crate::mm::vmm::vma::{VmaFlags, VmaPerm};
    use core::ptr;
    let base = crate::config::MM_SELFTEST_BASE as usize + 0x1_0000;

    let addr = vmm::mmap_anon(Some(base), 0x2000, VmaPerm::read_write(), VmaFlags { anonymous: true, ..VmaFlags::empty() }).expect("mmap");
    // 页尚未映射
    check!(vmm::with_vmm(|vm| vm.mapper.translate(crate::arch::x86_64::memory::VirtAddr(addr as u64)).is_none()).unwrap_or(false));

    // 写入 → 触发缺页 → 按需分配
    unsafe {
        ptr::write_volatile(addr as *mut u64, 0x4158_4953);
    }
    let v = unsafe { ptr::read_volatile(addr as *const u64) };
    check!(v == 0x4158_4953);
    // 已映射
    check!(vmm::with_vmm(|vm| vm.mapper.translate(crate::arch::x86_64::memory::VirtAddr(addr as u64)).is_some()).unwrap_or(false));

    // 第二页仍未映射（按需一页一页给）
    check!(vmm::with_vmm(|vm| vm.mapper.translate(crate::arch::x86_64::memory::VirtAddr((addr + 0x1000) as u64)).is_none()).unwrap_or(false));

    check!(vmm::munmap(addr, 0x2000).is_ok());
    true
}

/// 6) COW：共享只读 → 写入触发拆解，双方独立
fn selftest_cow() -> bool {
    use crate::arch::x86_64::memory::VirtAddr as VA;
    use crate::arch::x86_64::paging::PageTableFlags as PTF;
    use crate::mm::vmm::cow;
    use crate::mm::vmm::page_table::MappingFlags;
    use crate::mm::vmm::vma::{VmaFlags, VmaPerm};
    use core::ptr;

    let base = crate::config::MM_SELFTEST_BASE as usize + 0x3_0000;
    // a：立即映射（有自己的物理页）
    vmm::mmap_anon_eager(base, 0x1000, VmaPerm::read_write(), VmaFlags { anonymous: true, ..VmaFlags::empty() }).expect("mmap a");
    // b：仅登记 VMA，页由 cow_share 直接填共享帧
    vmm::mmap_anon(Some(base + 0x1000), 0x1000, VmaPerm::read_write(), VmaFlags { anonymous: true, ..VmaFlags::empty() }).expect("mmap b");
    let (a, b) = (base, base + 0x1000);

    unsafe { ptr::write_volatile(a as *mut u64, 0xC0FE_BA5E) };

    // 让 b 与 a 共享同一物理页，双方只读 + COW
    let shared_ok = vmm::with_vmm(|vm| {
        cow::cow_share(
            &mut vm.mapper,
            a,
            b,
            MappingFlags {
                present: true, writable: true, executable: false,
                user: false, cow: false, global: false,
            },
        )
        .is_ok()
    })
    .unwrap_or(false);
    check!(shared_ok);

    // 编译器栅栏：LTO 下优化器可以看到 cow_share 从不读写页内容，
    // 可能把下面的 volatile 写乱序到“共享前”执行（那时页还可写，
    // 缺页不会被触发，测试失_效）。fence 阻止这种跨函数重排。
    core::sync::atomic::compiler_fence(core::sync::atomic::Ordering::SeqCst);

    // 共享期间两个 VA → 同一物理页，且都只读
    let pa = vmm::with_vmm(|vm| vm.mapper.translate(VA(a as u64))).flatten();
    let pb = vmm::with_vmm(|vm| vm.mapper.translate(VA(b as u64))).flatten();
    check!(pa.is_some() && pb.is_some() && pa == pb, "both addrs should share one frame");

    // 写 a → 缺页（只读冲突）→ 拆解：a 独立可写。
    // 用内联汇编执行写入：asm 自身是不可重排的执行屏障，避免 LTO
    // 把写入搬到 cow_share 之前（那时页还可写，不会触发缺页）。
    unsafe {
        core::arch::asm!(
            "mov qword ptr [{0}], {1}",
            in(reg) a,
            in(reg) 0xAAF0_0001u64,
            options(nostack, preserves_flags)
        );
    }
    let a_writable = vmm::with_vmm(|vm| {
        vm.mapper
            .pte_flags(VA(a as u64))
            .map(|f| f.contains(PTF::WRITABLE))
            .unwrap_or(false)
    })
    .unwrap_or(false);
    check!(a_writable, "page A should be writable after break");

    // b 仍是 COW 只读，且内容还是拆解前的快照值
    let b_cow = vmm::with_vmm(|vm| {
        vm.mapper
            .pte_flags(VA(b as u64))
            .map(|f| f.contains(PTF::COW))
            .unwrap_or(false)
    })
    .unwrap_or(false);
    check!(b_cow, "page B should still be COW");
    let vb = unsafe { ptr::read_volatile(b as *const u64) };
    check!(vb == 0xC0FE_BA5E, "page B content should be the pre-break snapshot");

    // 写 b → 拆解 b：a/b 物理页从此不同
    unsafe { ptr::write_volatile(b as *mut u64, 0xBEEF_0002) };
    let pa2 = vmm::with_vmm(|vm| vm.mapper.translate(VA(a as u64))).flatten();
    let pb2 = vmm::with_vmm(|vm| vm.mapper.translate(VA(b as u64))).flatten();
    check!(pa2.is_some() && pb2.is_some() && pa2 != pb2, "pages should be independent after break");

    check!(vmm::munmap(a, 0x1000).is_ok());
    check!(vmm::munmap(b, 0x1000).is_ok());
    true
}

/// 7) 交换：swap_out → 缺页 → swap_in 还原内容
fn selftest_swap() -> bool {
    use crate::mm::vmm::vma::{VmaFlags, VmaPerm};
    use core::ptr;
    use crate::arch::x86_64::memory::VirtAddr as VA;

    let base = crate::config::MM_SELFTEST_BASE as usize + 0x4_0000;
    let addr = base;
    vmm::mmap_anon_eager(addr, 0x1000, VmaPerm::read_write(), VmaFlags { anonymous: true, ..VmaFlags::empty() }).expect("mmap swap");
    unsafe { ptr::write_volatile(addr as *mut u64, 0xDEAD_BEEF) };

    check!(vmm::swap_out_page(addr));
    // 换出后页表应无映射
    check!(vmm::with_vmm(|vm| vm.mapper.translate(VA(addr as u64)).is_none()).unwrap_or(false));

    // 读 → 缺页 → 自动换入 → 内容还原
    let v = unsafe { ptr::read_volatile(addr as *const u64) };
    check!(v == 0xDEAD_BEEF);
    check!(vmm::with_vmm(|vm| vm.mapper.translate(VA(addr as u64)).is_some()).unwrap_or(false));

    check!(vmm::munmap(addr, 0x1000).is_ok());
    true
}