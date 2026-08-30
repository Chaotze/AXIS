; ============================================================
; x86_64 内核引导入口（高半核映射）
; ============================================================
; 由 Bootloader（BIOS Stage2 或 UEFI）在已进入长模式后跳转而来。
; 整个引导过程分为两个阶段，分别运行在不同的地址空间：
;
;   阶段一：低地址引导桩（section .boot，虚拟地址 = 物理地址 = 0x100000）
;     1. 加载内核自己的平坦 GDT，重载段寄存器
;     2. 建立三类页表映射（详见 init_page_tables）：
;        a. PML4[0]   低端恒等映射 [0, 4GB)   —— 临时，仅保证引导桩可持续执行
;        b. PML4[256] 物理内存映射区           —— 长期，供内核访问硬件 MMIO
;        c. PML4[511] 内核高半区映射           —— 长期，内核主体真正的位置
;     3. 切换 CR3，确保 PAE / NXE 已启用
;     4. 绝对跳转到高半区的跳板 higher_half
;   阶段二：高地址跳板（section .text，高半区虚拟地址）
;     5. 切换高半区引导栈，清零 BSS
;     6. 删除 PML4[0] 恒等映射（取消低端映射）
;     7. 移交 Rust 内核入口 _boot_rust
;
; 为什么需要两阶段（低桩 + 高板）：
;   - 引导桩必须运行在"虚拟地址 = 物理地址"的低地址：Bootloader 按 ELF 的
;     e_entry（= 0x100000，物理地址）直接跳转进来，此刻高半区页表尚未建立，
;     无法以高地址取指
;   - 内核主体（Rust 代码与数据）链接在高半区，必须先建好页表再绝对跳转，
;     才能让 RIP 进入高地址
;   - 恒等映射必须在"绝对跳转之后"才能删除：删除前，引导桩自身正以低地址
;     取指执行，PML4[0] 一旦置零，下一条指令立即缺页
;
; 为什么跳转后要删除低端恒等映射：
;   - 保留它会让内核能访问"未纳入设计"的低端虚拟地址，破坏虚拟地址空间的
;     干净划分；用户态进程将来还要占用低端 4GB，更不允许内核地址混入
;   - Local APIC（0xFEE00000）/ I/O APIC（0xFEC00000）/ VGA（0xB8000）等
;     MMIO 改经物理内存映射区访问（见阶段二第 6 步的注释）
;
; Intel 语法，使用 NASM 汇编器

; ============================================================
; 常量定义
; ============================================================

; 内核虚拟基地址（高半核），与 kernel.ld 的 KERNEL_VIRT_BASE 保持一致
%define KERNEL_VIRT_BASE 0xFFFFFFFF80000000

; 物理内存映射区偏移，与 config.rs 的 PHYSICAL_MEMORY_OFFSET 保持一致
; （物理地址 P 的 MMIO 一律以 P + 该偏移 的虚拟地址访问）
%define PHYS_MEM_OFFSET 0xFFFF800000000000

; 页表大小和掩码
%define PAGE_SIZE 4096
%define PML4_SIZE 512
%define PDPT_SIZE 512
%define PD_SIZE 512
%define PT_SIZE 512

; 页表项标志位
%define PTE_PRESENT    (1 << 0)   ; P - 存在位
%define PTE_WRITABLE   (1 << 1)   ; R/W - 可写位
%define PTE_HUGE       (1 << 7)   ; PS - 2MB 巨页
%define PTE_KERNEL_PAGE (PTE_PRESENT | PTE_WRITABLE | PTE_HUGE) ; 2MB 巨页表项
%define PTE_KERNEL     (PTE_PRESENT | PTE_WRITABLE)             ; 页表目录项

; 页表索引常量（由地址的位 47-39 / 38-30 计算得到）
%define PML4_IDX_IDENT   0      ; 低端恒等映射（虚拟 [0, 512GB)）
%define PML4_IDX_PHYS  256      ; 物理内存映射区（0xFFFF800000000000 >> 39）
%define PML4_IDX_KERNEL 511     ; 内核高半区（0xFFFFFFFF80000000 >> 39）
%define PDPT_IDX_KERNEL 510     ; KERNEL_VIRT_BASE 所在的 1GB 窗口

; MSR 常量
%define MSR_EFER 0xC0000080      ; Extended Feature Enable Register
%define EFER_LME  (1 << 8)       ; Long Mode Enable
%define EFER_NXE  (1 << 11)      ; No-Execute Enable

; CR4 标志位
%define CR4_PAE   (1 << 5)       ; Physical Address Extension

; ============================================================
; 64 位入口段（低地址引导桩）
; ============================================================

section .boot
bits 64

global _boot
extern _boot_rust
extern __bss_start
extern __bss_end
extern __kernel_phys_end    ; 由 kernel.ld 导出的内核映像物理末端

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
    ; 第二步：建立高半核页表
    ; --------------------------------------------------------
    ; 一次性建立三类映射（详见 init_page_tables 内的注释），
    ; 以便"切页表 --> 跳高地址 --> 删恒等映射"一气呵成。
    call init_page_tables

    ; 切换到内核自己的页表（写 CR3 同时刷新所有非全局 TLB 项）
    ; 新页表仍含低端恒等映射，因此当前低地址位置的代码可继续执行
    lea rax, [rel pml4_table]
    mov cr3, rax

    ; 确保 PAE 已启用（长模式强制要求；Stage2 已设置，这里幂等再设）
    mov rax, cr4
    or rax, CR4_PAE
    mov cr4, rax

    ; 启用 NXE 和 LME（NXE 供后续页表 NX 位生效；
    ; LME 在进入长模式后由 CPU 自动转成 LMA，保留设置无害）
    mov ecx, MSR_EFER
    rdmsr
    or eax, EFER_NXE | EFER_LME
    wrmsr

    ; --------------------------------------------------------
    ; 第三步：绝对跳转到高半区跳板
    ; --------------------------------------------------------
    ; 为什么用"绝对寻址 + 寄存器间接跳转"：
    ;   - higher_half 位于高半区虚拟地址，与当前 RIP（低地址）相距超过 ±2GB，
    ;     RIP 相对寻址（32 位位移）无法表达
    ;   - mov rax, imm64 由链接器把符号解析为高半区绝对地址填进立即数
    mov rax, higher_half
    jmp rax

; ============================================================
; 页表初始化（在启用新页表、跳转高地址之前调用）
; ============================================================
; 此时仍在 Stage2 的 0-2GB 恒等映射下运行，页表区位于内核映像
; 的低端 .boot 段（物理 0x100000+），可直接按物理地址写入。
;
; 页表结构（低端 .boot 段，共 5 页表 = 32KB）：
;   PML4[0]   -> pdpt_table（共享）       恒等映射 [0, 4GB)
;   PML4[256] -> pdpt_table（共享）       物理内存映射区 [0xFFFF800000000000, +4GB)
;   PML4[511] -> pdpt_kernel[510] -> pd_kernel   内核高半区映射
;
; 为什么恒等映射与物理内存映射区共享同一个 PDPT：
;   - 两者覆盖的物理范围完全一致（[0, 4GB)），PDPT 内容（指向相同的
;     4 张 PD 页）完全相同，共享一份即可省下 4KB
;   - 删除恒等映射时只需置零 PML4[0]，物理内存映射区不受影响
init_page_tables:
    ; 清零页表区域：PML4(4KB) + PDPT(4KB) + PD(16KB) + PDPT_K(4KB) + PD_K(4KB)
    lea rdi, [rel pml4_table]
    mov ecx, 0x8000 / 8
    xor eax, eax
    rep stosq

    ; PML4[0] = pdpt_table | P | RW             （低端恒等映射，临时）
    lea rax, [rel pdpt_table]
    or eax, PTE_KERNEL
    mov [rel pml4_table + PML4_IDX_IDENT*8], rax

    ; PML4[256] = 同一个 pdpt_table | P | RW    （物理内存映射区，长期）
    ; 注意：这里直接复用 rax（仍是 pdpt_table 地址与标志），无需重新计算
    mov [rel pml4_table + PML4_IDX_PHYS*8], rax

    ; PML4[511] = pdpt_kernel | P | RW          （内核高半区）
    lea rax, [rel pdpt_kernel]
    or eax, PTE_KERNEL
    mov [rel pml4_table + PML4_IDX_KERNEL*8], rax

    ; PDPT[0..3] = 指向 4 张 PD 页（共享，覆盖 4GB）
    ; （4 个条目各指向一个 PD 页，每页 512 个巨页项，共 2048 × 2MB = 4GB）
    lea rbx, [rel pd_table]
    lea rdx, [rel pdpt_table]
    mov ecx, 4
.init_pdpt_loop:
    mov rax, rbx
    or eax, PTE_KERNEL
    mov [rdx], rax
    add rbx, 0x1000
    add rdx, 8
    dec ecx
    jnz .init_pdpt_loop

    ; PDPT_K[510] = pd_kernel | P | RW
    ; （510 号 1GB 窗口正是 KERNEL_VIRT_BASE 所在位置）
    lea rax, [rel pd_kernel]
    or eax, PTE_KERNEL
    mov [rel pdpt_kernel + PDPT_IDX_KERNEL*8], rax

    ; PD 表：2048 个 2MB 巨页，映射物理 [0, 4GB)
    ; 同时服务 PML4[0]（恒等）与 PML4[256]（物理内存映射区）两棵路径
    lea rdi, [rel pd_table]
    mov eax, PTE_KERNEL_PAGE    ; 第一项：物理 0
    mov ecx, 2048
.init_pd_loop:
    mov [rdi], rax
    add eax, 0x200000           ; 下一个 2MB 页
    add rdi, 8
    dec ecx
    jnz .init_pd_loop

    ; PD_K 表：按内核映像实际大小映射高半区
    ; 条目 i 把虚拟地址 (KERNEL_VIRT_BASE + i*2MB) 映射到物理 (i*2MB)，
    ; 与 kernel.ld 中"高半区段 VMA = LMA + KERNEL_VIRT_BASE"的布局一一对应
    mov rax, __kernel_phys_end          ; 内核映像物理末端（链接常量）
    add rax, (1 << 21) - 1
    shr rax, 21                         ; 向上取整到 2MB 页数
    ; 单张 PD 页只有 512 项（最多覆盖 1GB），而 KERNEL_VIRT_BASE 之上
    ; 1GB 处即内核堆（config.rs 布局），内核映像不可能超过该上限
    cmp rax, PD_SIZE
    ja .kernel_image_too_large

    lea rdi, [rel pd_kernel]
    mov rbx, 0                          ; 当前 2MB 页的物理基址
.init_pd_kernel_loop:
    test rax, rax
    jz .init_pd_kernel_done
    mov r8, rbx
    or r8, PTE_KERNEL_PAGE
    mov [rdi], r8
    add rbx, 0x200000
    add rdi, 8
    dec rax
    jmp .init_pd_kernel_loop
.init_pd_kernel_done:
    ret

.kernel_image_too_large:
    ; 内核映像超过 1GB（异常情况，正常构建不可能发生）——挂起报错
.hang_too_large:
    hlt
    jmp .hang_too_large

; ============================================================
; 高地址跳板（section .text，链接于高半区）
; ============================================================
; 这是内核在"高半区虚拟地址"上执行的第一段代码，由低地址引导桩
; 绝对跳转而来。此时三类映射均已生效，但引导栈仍在低端（Stage2
; 遗留的 RSP = 0x200000），必须在删除恒等映射之前完成栈切换。

section .text
bits 64

higher_half:
    ; --------------------------------------------------------
    ; 第四步：切换高半区引导栈
    ; --------------------------------------------------------
    ; Stage2 遗留的 RSP = 0x200000 属于低端恒等映射，即将被删除；
    ; 立即换到高半区 .bss 上的引导栈（同处高半区，RIP 相对寻址可及）。
    ; 切换后低端栈即使被删除也不再影响执行。
    lea rsp, [rel boot_stack_top]
    xor rbp, rbp                ; RBP = 0，标识栈帧的底部

    ; --------------------------------------------------------
    ; 第五步：清零高半区 BSS
    ; --------------------------------------------------------
    ; Rust 的 static 变量（GDT/IDT/TSS/自旋锁等）大多位于 .bss，
    ; 标准要求未初始化全局变量必须为 0。Bootloader 的 ELF 加载器
    ; 已清过一次，这里再清一次以保证幂等。
    ; 为什么必须在此处（高地址）清：__bss_start/__bss_end 位于高半区，
    ; 低地址代码无法用 RIP 相对寻址访问它们。
    lea rdi, [rel __bss_start]
    lea rcx, [rel __bss_end]
    sub rcx, rdi
    xor eax, eax
    rep stosb

    ; --------------------------------------------------------
    ; 第六步：取消恒等映射，删除低端映射
    ; --------------------------------------------------------
    ; 现在代码已在高半区执行，可以安全地置零 PML4[0]。
    ;
    ; 为什么通过"物理内存映射区"访问页表：
    ;   - PML4 位于低端 .boot 段（物理 0x100000+），置零自身条目后
    ;     低端虚拟地址立即失效，不能再以低地址读写它
    ;   - 物理内存映射区（PML4[256]）仍覆盖该页，以
    ;     物理地址 + PHYS_MEM_OFFSET 的虚拟地址访问即可绕开低端映射
    mov rax, pml4_table             ; 取 PML4 的物理地址（= 低端虚拟地址）
    mov rbx, PHYS_MEM_OFFSET        ; mov 支持 imm64；add 的 imm32 会截断，故走寄存器
    add rax, rbx                    ; 转换为物理内存映射区中的虚拟地址
    mov qword [rax + PML4_IDX_IDENT*8], 0    ; 删除 PML4[0] 整棵子树

    ; 刷新 TLB：重载 CR3 使上述删除对所有后续访问立即生效
    mov rax, cr3
    mov cr3, rax

    ; --------------------------------------------------------
    ; 第七步：移交 Rust 内核
    ; --------------------------------------------------------
    ; _boot_rust 声明为 -> !，理论上不会返回；防御性挂起兜底。
    ; 参数按 System V AMD64 ABI：RDI = 引导信息指针（暂无，置 0）
    xor edi, edi
    call _boot_rust

.hang:
    hlt
    jmp .hang

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
    resq 512                  ; PDPT：512 项（由 PML4[0] 与 PML4[256] 共享）

pd_table:
    resq 2048                 ; 4 张 PD 页：2048 项 × 8 字节 = 16KB
                              ; （2MB 巨页 × 2048 = 4GB 恒等 / 物理内存映射）

pdpt_kernel:
    resq 512                  ; 内核高半区 PDPT：512 项 × 8 字节 = 4KB

pd_kernel:
    resq 512                  ; 内核高半区 PD：512 × 2MB = 最多 1GB
                              ; （KERNEL_VIRT_BASE 之上 1GB 处即内核堆）

; ============================================================
; 引导栈（高半区 .bss，由 higher_half 在清零 BSS 后设置）
; ============================================================

section .bss
align 16

boot_stack_bottom:
    ; 256KB 引导栈
    ; 为什么从 64KB 增大：mm 自测（COW/交换）与未来的任务
    ; 子系统初始化调用链较深，64KB 在深层嵌套时余量不足；
    ; 栈溢出会静默破坏相邻 .bss 数据（如 Vec 元数据），
    ; 表现为难以定位的空指针缺页
    resb 0x40000
boot_stack_top: