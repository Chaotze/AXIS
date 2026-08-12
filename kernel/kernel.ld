/* ============================================================
 * AXIS 内核链接脚本
 * ============================================================
 * 定义内核的内存布局
 *
 * 内核加载地址：1MB（0x100000）
 * 这是传统的内核加载位置，避开低内存区域
 */

OUTPUT_FORMAT(elf64-x86-64)
OUTPUT_ARCH(i386:x86-64)
ENTRY(_start)

/* 内核虚拟地址基址 */
KERNEL_VIRT_BASE = 0xFFFFFFFF80000000;

/* 内核物理地址基址 */
KERNEL_PHYS_BASE = 0x100000;

SECTIONS
{
    /* 内核从 1MB 开始 */
    . = KERNEL_PHYS_BASE;

    /* 内核起始地址（物理） */
    __kernel_start = .;

    /* 代码段 */
    .text : AT(ADDR(.text) - KERNEL_VIRT_BASE)
    {
        /* 确保入口点在最前面 */
        *(.text._start)
        *(.text .text.*)
    }

    /* 只读数据段 */
    .rodata : ALIGN(4096) AT(ADDR(.rodata) - KERNEL_VIRT_BASE)
    {
        *(.rodata .rodata.*)
    }

    /* 可读写数据段 */
    .data : ALIGN(4096) AT(ADDR(.data) - KERNEL_VIRT_BASE)
    {
        *(.data .data.*)
    }

    /* BSS 段（未初始化数据） */
    .bss : ALIGN(4096) AT(ADDR(.bss) - KERNEL_VIRT_BASE)
    {
        __bss_start = .;
        *(.bss .bss.*)
        *(COMMON)
        __bss_end = .;
    }

    /* 内核结束地址（物理） */
    __kernel_end = .;

    /* 丢弃不需要的段 */
    /DISCARD/ :
    {
        *(.eh_frame)
        *(.comment)
        *(.note*)
    }
}
