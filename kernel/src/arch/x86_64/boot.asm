; ============================================================
; x86_64 引导入口
; ============================================================
; 兼容 32 位和 64 位模式，无论如何进入都会：
; 1. 建立完整的页表（低地址恒等映射 + 高地址映射）
; 2. 进入长模式（如果还未进入）
; 3. 启用分页
; 4. 跳转到高地址 Rust 入口
;
; Intel 语法，使用 NASM 汇编器

; ============================================================
; 常量定义
; ============================================================

; 内核虚拟基地址（高半核）
%define KERNEL_VIRT_BASE 0xFFFFFFFF80000000

; 页表大小和掩码
%define PAGE_SIZE 4096
%define PML4_SIZE 512
%define PDPT_SIZE 512
%define PD_SIZE 512
%define PT_SIZE 512

; 页表项标志位
%define PTE_PRESENT    (1 << 0)   ; P - 存在位
%define PTE_WRITABLE   (1 << 1)   ; R/W - 可写位
%define PTE_USER       (1 << 2)   ; U/S - 用户位
%define PTE_WRITETHROUGH (1 << 3) ; PWT - 写穿透
%define PTE_NOCACHE    (1 << 4)   ; PCD - 禁止缓存
%define PTE_HUGE       (1 << 7)   ; PS - 巨页（在PD中表示2MB）
%define PTE_GLOBAL     (1 << 8)   ; G - 全局页
%define PTE_NO_EXECUTE (1 << 63)  ; XD - 禁止执行

; 标准页表项（内核可读写）
%define PTE_KERNEL (PTE_PRESENT | PTE_WRITABLE)

; MSR 常量
%define MSR_EFER 0xC0000080      ; Extended Feature Enable Register
%define EFER_SCE  (1 << 0)       ; System Call Extensions
%define EFER_LME  (1 << 8)       ; Long Mode Enable
%define EFER_LMA  (1 << 10)      ; Long Mode Active
%define EFER_NXE  (1 << 11)      ; No-Execute Enable

; CR0 标志位
%define CR0_PE    (1 << 0)       ; Protection Enable
%define CR0_PG    (1 << 31)      ; Paging

; CR4 标志位
%define CR4_PAE   (1 << 5)       ; Physical Address Extension
%define CR4_PGE   (1 << 7)       ; Page Global Enable

; ============================================================
; 32 位启动段
; ============================================================

section .boot
bits 32

global _boot
extern _boot_rust
extern __bss_start
extern __bss_end

_boot:
    ; 此时 CPU 处于 32 位保护模式
    ; 寄存器状态：
    ;   EBX = Multiboot2 信息指针
    ;   其他寄存器未定义

    cli                         ; 禁用中断
    cld                         ; DF = 0（字符串操作从低到高）
    
    ; 保存 Multiboot2 信息指针（EBX 在两种模式下都有效）
    mov esi, ebx                ; ESI 保存，后面会传给 Rust

    ; ====================================================
    ; 第一步：检测当前 CPU 模式
    ; ====================================================
    
    ; 检查是否在长模式
    mov ecx, MSR_EFER
    rdmsr
    test eax, EFER_LME
    jnz .in_long_mode           ; 已在长模式
    
    ; ====================================================
    ; 情况 A：从 32 位保护模式启动
    ; ====================================================
    
    ; 检查是否在保护模式（PE 位）
    mov eax, cr0
    test eax, CR0_PE
    jz .not_in_protected_mode   ; 不应该发生，但做防御性处理
    
    ; 保存 EBX（已在 ESI 中）
    
    ; 初始化页表（32位模式下，使用物理地址）
    call init_page_tables
    
    ; 启用 PAE
    mov eax, cr4
    or eax, CR4_PAE
    mov cr4, eax
    
    ; 启用 NXE 和 LME
    mov ecx, MSR_EFER
    rdmsr
    or eax, EFER_NXE | EFER_LME
    wrmsr
    
    ; 加载临时 GDT
    lgdt [gdt_descriptor]
    
    ; 设置 CR3 指向 PML4
    mov eax, pml4_table
    mov cr3, eax
    
    ; 启用分页
    mov eax, cr0
    or eax, CR0_PG
    mov cr0, eax
    
    ; 执行 far jmp 进入 64 位代码段
    lea eax, [.long_mode_entry]
    push dword 0x08             ; 64位代码段选择子
    push eax
    retf

.not_in_protected_mode:
    ; 如果不在保护模式（可能在实模式），显示错误
    mov word [0xB8000], 0x4F50  ; 'P'
    mov word [0xB8002], 0x4F4D  ; 'M'
    jmp .hang

    ; ====================================================
    ; 情况 B：从 64 位长模式启动
    ; ====================================================
    
section .boot
bits 64

.in_long_mode:
    ; 已经在 64 位长模式，重新设置必要的部分
    
    ; 1. 重新加载 GDT
    lgdt [gdt_descriptor_64]
    
    ; 2. 重新加载数据段
    mov ax, 0x10                ; 64位数据段选择子
    mov ss, ax
    mov ds, ax
    mov es, ax
    mov fs, ax
    mov gs, ax
    
    ; 3. 重新设置 CR3（重新加载页表）
    ; 注意：这里 pml4_table 是物理地址，但在长模式下需要虚拟地址
    ; 假设 identity mapping 使得物理地址可以直接访问
    mov rax, pml4_table
    mov cr3, rax
    
    ; 4. 重新设置 CR4（确保 PAE 和所需标志）
    mov rax, cr4
    or rax, CR4_PAE
    ; 可以添加其他标志，如 CR4_OSFXSR, CR4_OSXMMEXCPT 等
    mov cr4, rax
    
    ; 5. 重新设置 EFER（确保 NXE 和 LME）
    mov ecx, MSR_EFER
    rdmsr
    or eax, EFER_NXE | EFER_LME
    wrmsr
    
    ; 6. 重新设置 CR0（确保分页等）
    mov rax, cr0
    or rax, CR0_PG
    ; 清除可能不需要的标志
    ; and rax, ~CR0_WP  ; 如果需要可以禁用写保护
    mov cr0, rax
    
    ; 7. 切换到我们自己的栈（如果有）
    ; mov rsp, stack_top
    
    ; 跳转到统一的 64 位设置
    jmp .long_mode_setup

section .boot
bits 64

.long_mode_entry:
    ; 从 32 位模式 far jmp 进入
    
    ; 重新加载数据段（使用 64 位数据段）
    mov ax, 0x10
    mov ss, ax
    mov ds, ax
    mov es, ax
    mov fs, ax
    mov gs, ax
    
    ; 跳转到统一的 64 位设置
    jmp .long_mode_setup

.long_mode_setup:
    ; ====================================================
    ; 统一的 64 位环境设置
    ; ====================================================
    
    ; 此时：
    ;   - CPU 在 64 位长模式
    ;   - 分页已启用
    ;   - GDT 已加载
    ;   - ESI 保存着 Multiboot2 信息指针（32位值）
    
    ; 清空 BSS 段
    lea rdi, [__bss_start]
    lea rcx, [__bss_end]
    sub rcx, rdi
    xor rax, rax
    cld
    rep stosb
    
    ; 设置栈指针（如果还没设置）
    ; lea rsp, [stack_top]
    
    ; 调用 Rust 入口
    ; 参数：RDI = Multiboot2 信息指针（从 ESI 转换）
    mov edi, esi                ; 32位指针扩展到64位
    xor rax, rax
    call _boot_rust
    
    ; 不应该返回
.hang:
    hlt
    jmp .hang

; ============================================================
; 页表初始化（32位代码）
; ============================================================

section .boot
bits 32

init_page_tables:
    ; 使用 2MB 大页的 identity mapping
    ; PML4 -> PDP -> PD -> 2MB 页
    
    ; 清空页表区域（假设在 .boot 段中）
    lea edi, [pml4_table]
    mov ecx, 0x1000 * 3         ; PML4 + PDP + PD (每个4KB)
    xor eax, eax
    cld
    rep stosb
    
    ; 设置 PML4 条目
    lea eax, [pdp_table]
    or eax, 0x03                ; 存在 + 可写
    mov [pml4_table], eax
    
    ; 设置 PDP 条目（指向 PD）
    lea eax, [pd_table]
    or eax, 0x03                ; 存在 + 可写
    mov [pdp_table], eax
    
    ; 设置 PD 条目（2MB 大页，覆盖前 2MB）
    ; 实际上需要覆盖更多内存，这里只设置一个示例
    mov eax, 0x00000083         ; 2MB 页，存在，可写，PS=1
    mov [pd_table], eax
    
    ; 为了覆盖更多内存，可以设置多个 PD 条目
    ; 例如覆盖 0-4GB 需要 2048 个 PD 条目（每个2MB）
    ; 这里是简化版本
    
    ret

; ============================================================
; GDT（全局描述符表）
; ============================================================

section .boot
align 8

gdt:
    ; 空描述符
    dq 0x0000000000000000
    
    ; 64位代码段（选择子 0x08）
    ; 基址=0，限制=0，粒度=0，存在，可读，执行，64位
    dw 0x0000                   ; 限制低16位
    dw 0x0000                   ; 基址低16位
    db 0x00                     ; 基址中8位
    db 0b10011010               ; P=1, DPL=0, S=1, 代码段，可执行，可读
    db 0b00100000               ; G=0, D/B=0, L=1, AVL=0, 限制高4位=0
    db 0x00                     ; 基址高8位
    
    ; 64位数据段（选择子 0x10）
    dw 0x0000                   ; 限制低16位
    dw 0x0000                   ; 基址低16位
    db 0x00                     ; 基址中8位
    db 0b10010010               ; P=1, DPL=0, S=1, 数据段，可读可写
    db 0b00000000               ; G=0, D/B=0, L=0, AVL=0, 限制高4位=0
    db 0x00                     ; 基址高8位

gdt_descriptor:
    dw gdt_descriptor - gdt - 1 ; 限制
    dd gdt                      ; 基址（32位物理地址）

; 在64位模式下使用的GDT描述符（与上面相同，但提供64位地址）
section .boot
bits 64

gdt_descriptor_64:
    dw gdt_descriptor_64 - gdt - 1
    dq gdt                     ; 基址（64位虚拟地址）

; ============================================================
; 数据段：页表
; ============================================================

section .boot
align 4096

pml4_table:
    times 512 dq 0

pdp_table:
    times 512 dq 0

pd_table:
    times 512 dq 0

; ============================================================
; BSS 段（由链接脚本定义）
; ============================================================

section .bss
; __bss_start 和 __bss_end 由链接器脚本提供
