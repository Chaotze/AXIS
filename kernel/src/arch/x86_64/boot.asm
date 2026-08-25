; ============================================================
; x86_64 内核引导入口（64 位）
; ============================================================
; 由 Bootloader（BIOS Stage2 或 UEFI）在已进入长模式后跳转而来，
; 职责：
;   1. 加载内核自己的平坦 GDT，重载段寄存器
;   2. 建立 0-4GB 恒等映射页表（2MB 巨页），接管 CR3
;   3. 确保 PAE / LME / NXE 已启用
;   4. 清零 BSS，设置引导栈
;   5. 跳转到 Rust 内核入口 _boot_rust
;
; 为什么这里只处理 64 位入口：
;   - 本项目的 BIOS Stage2 与 UEFI 引导器在跳转内核前都已进入长模式
;   - AXIS 已取消 Multiboot2 协议支持（见提交 c17258c），
;     不存在 32 位保护模式直接进入内核的路径
;   - 内核链接在低地址 0x100000（kernel.ld 中 KERNEL_VIRT_BASE = 0），
;     恒等映射下物理地址 = 虚拟地址，Rust 代码可直接运行
;
; 为什么必须映射整个 0-4GB 而不是只有内核占用的前几 MB：
;   - Local APIC 寄存器位于 0xFEE00000，I/O APIC 位于 0xFEC00000
;   - 中断系统初始化（apic.rs / ioapic.rs）会直接访问这些 MMIO 地址
;   - 如果页表只覆盖低地址，首次访问 APIC 就会触发页错误
;
; Intel 语法，使用 NASM 汇编器

; ============================================================
; 常量定义
; ============================================================

; 页表项标志位
%define PTE_PRESENT    (1 << 0)   ; P - 存在位
%define PTE_WRITABLE   (1 << 1)   ; R/W - 可写位
%define PTE_HUGE       (1 << 7)   ; PS - 2MB 巨页
%define PTE_KERNEL_PAGE (PTE_PRESENT | PTE_WRITABLE | PTE_HUGE)

; MSR 常量
%define MSR_EFER 0xC0000080      ; Extended Feature Enable Register
%define EFER_LME  (1 << 8)       ; Long Mode Enable
%define EFER_NXE  (1 << 11)      ; No-Execute Enable

; CR4 标志位
%define CR4_PAE   (1 << 5)       ; Physical Address Extension

; ============================================================
; 64 位入口段
; ============================================================

section .boot
bits 64

global _boot
extern _boot_rust
extern __bss_start
extern __bss_end

_boot:
    ; 进入时的 CPU 状态（由 BIOS Stage2 保证）：
    ;   - 已处于 64 位长模式，分页已启用（Stage2 的 0-2GB 恒等映射）
    ;   - CR3 指向 Stage2 的临时页表（位于物理 0x1000）
    ;   - CS = 0x08（Stage2 的 64 位代码段），RSP = 0x200000
    cli                         ; 禁用中断，初始化期间不允许被打断
    cld                         ; DF = 0，字符串操作从低地址向高地址

    ; --------------------------------------------------------
    ; 第一步：加载内核自己的平坦 GDT
    ; --------------------------------------------------------
    ; Stage2 的 GDT 位于 0x7e00 附近的引导内存中，内核启动后
    ; 随时可能被覆盖，因此必须切换到内核映像内部的 GDT。
    lgdt [rel gdt64_descriptor]

    ; 重载数据段寄存器（0x10 = 内核数据段）
    mov ax, 0x10
    mov ss, ax
    mov ds, ax
    mov es, ax
    mov fs, ax
    mov gs, ax

    ; 重载 CS：通过远返回将 0x08（内核代码段）写入 CS
    push 0x08
    lea rax, [rel .cs_reloaded]
    push rax
    retfq
.cs_reloaded:

    ; --------------------------------------------------------
    ; 第二步：建立 0-4GB 恒等映射页表
    ; --------------------------------------------------------
    ; 页表结构（全部位于内核映像的 .boot 段，物理地址 < 4GB）：
    ;   PML4[0]      -> PDPT
    ;   PDPT[0..3]   -> PD 的 4 个 4KB 页（共 2048 个 2MB 巨页项）
    ;   2048 × 2MB   = 4GB，覆盖 LAPIC/IOAPIC 等全部 MMIO 区域
    call init_page_tables

    ; 切换到内核自己的页表（写 CR3 同时刷新所有非全局 TLB 项）
    lea rax, [rel pml4_table]
    mov cr3, rax

    ; 确保 PAE 已启用（长模式强制要求；Stage2 已设置，这里幂等再设）
    mov rax, cr4
    or rax, CR4_PAE
    mov cr4, rax

    ; 启用 NXE 和 LME（NXE 供后续页表的 NX 位生效；
    ; LME 在进入长模式后由 CPU 自动转成 LMA，保留设置无害）
    mov ecx, MSR_EFER
    rdmsr
    or eax, EFER_NXE | EFER_LME
    wrmsr

    ; --------------------------------------------------------
    ; 第三步：清零 BSS 段
    ; --------------------------------------------------------
    ; Rust 的 static 变量（GDT/IDT/TSS/自旋锁等）大多位于 .bss，
    ; 标准要求未初始化全局变量必须为 0，Bootloader 的 ELF 加载器
    ; 虽已清过一次，这里再清一次以保证幂等。
    lea rdi, [rel __bss_start]
    lea rcx, [rel __bss_end]
    sub rcx, rdi
    xor eax, eax
    rep stosb

    ; --------------------------------------------------------
    ; 第四步：设置引导栈并跳入 Rust
    ; --------------------------------------------------------
    ; 引导栈放在 .bss 尾部区域（boot_stack_top），
    ; 不再依赖 Stage2 遗留的 RSP = 0x200000
    lea rsp, [rel boot_stack_top]
    xor rbp, rbp                ; RBP = 0，标识栈帧的底部

    ; 调用 Rust 入口（参数按 System V AMD64 ABI：RDI = 引导信息指针）
    ; 当前没有需要传递的引导信息，置 0
    xor edi, edi
    call _boot_rust

    ; _boot_rust 声明为 -> !，理论上不会返回；防御性挂起
.hang:
    hlt
    jmp .hang

; ============================================================
; 页表初始化（在启用新页表前调用）
; ============================================================
; 此时仍在 Stage2 的 0-2GB 恒等映射下运行，页表区位于内核映像
; （物理 0x100000+），可直接按物理地址写入。
init_page_tables:
    ; 清零页表区域：PML4(4KB) + PDPT(4KB) + 4 个 PD(16KB) = 24KB
    lea rdi, [rel pml4_table]
    mov ecx, 0x6000 / 8
    xor eax, eax
    rep stosq

    ; PML4[0] = PDPT | P | RW
    lea rax, [rel pdpt_table]
    or eax, PTE_PRESENT | PTE_WRITABLE
    mov [rel pml4_table], rax

    ; PDPT[0..3] = pd_table + i*4096 | P | RW
    ; （4 个条目各指向一个 PD 页，每页 512 个巨页项，共覆盖 4GB）
    lea rbx, [rel pd_table]
    lea rdx, [rel pdpt_table]
    mov ecx, 4
.init_pdpt_loop:
    mov rax, rbx
    or eax, PTE_PRESENT | PTE_WRITABLE
    mov [rdx], rax
    add rbx, 0x1000
    add rdx, 8
    dec ecx
    jnz .init_pdpt_loop

    ; PD 表：2048 个 2MB 巨页项，映射物理 [0, 4GB)
    ; 表项 = 页基址 | P | RW | PS
    lea rdi, [rel pd_table]
    mov eax, PTE_KERNEL_PAGE   ; 第一项：物理 0
    mov ecx, 2048
.init_pd_loop:
    mov [rdi], rax
    add eax, 0x200000          ; 下一个 2MB 页
    add rdi, 8
    dec ecx
    jnz .init_pd_loop
    ret

; ============================================================
; 64 位 GDT（临时，供 Rust 代码接管前使用）
; ============================================================
; gdt.rs 在 arch::init() 中会建立正式的 GDT/TSS 并再次 lgdt，
; 这里只需要平坦的代码段和数据段，保证内核代码可继续执行。

section .boot
align 8

gdt64:
    ; GDT[0] - 空描述符（CPU 要求）
    dq 0x0000000000000000
    ; GDT[1] - 64 位内核代码段（选择子 0x08）
    ; 基址 = 0，L = 1（长模式），P = 1，DPL = 0，可执行可读
    dq 0x00AF9A000000FFFF
    ; GDT[2] - 64 位内核数据段（选择子 0x10）
    ; 基址 = 0，P = 1，DPL = 0，可读可写
    dq 0x00CF92000000FFFF
gdt64_end:

gdt64_descriptor:
    dw gdt64_end - gdt64 - 1   ; GDT 界限
    dq gdt64                   ; GDT 基地址（64 位虚拟地址 = 物理地址）

; ============================================================
; 页表数据区（.boot 段，4KB 对齐）
; ============================================================

section .boot
align 4096

pml4_table:
    resq 512                  ; PML4：512 项 × 8 字节 = 4KB

pdpt_table:
    resq 512                  ; PDPT：512 项 × 8 字节 = 4KB

pd_table:
    resq 2048                 ; 4 个 PD 页：2048 项 × 8 字节 = 16KB
                              ; （2MB 巨页 × 2048 = 4GB 恒等映射）

; ============================================================
; 引导栈（.bss，由 _boot 在清零 BSS 后设置）
; ============================================================

section .bss
align 16

boot_stack_bottom:
    resb 0x10000              ; 64KB 引导栈
boot_stack_top:
