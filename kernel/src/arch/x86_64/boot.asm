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

    ; ====================================================
    ; 第一步：检查当前 CPU 模式
    ; ====================================================

    ; 尝试读取 EFER MSR（只有在 x86_64 上才支持）
    mov ecx, MSR_EFER
    rdmsr                       ; 读入 EAX:EDX
    test eax, EFER_LME          ; 检查 LME 位
    jnz .already_long_mode      ; 如果已在长模式，跳过32位初始化

    ; ====================================================
    ; 第二步：从 32 位进入 64 位
    ; ====================================================

    ; 保存 EBX（Multiboot2 信息指针）
    mov esi, ebx

    ; 初始化页表
    call init_page_tables

    ; 启用 PAE（Physical Address Extension）- 必须在启用长模式之前
    mov eax, cr4
    or eax, CR4_PAE
    mov cr4, eax

    ; 启用 NXE（No-Execute）和 LME（Long Mode Enable）
    mov ecx, MSR_EFER
    rdmsr
    or eax, EFER_NXE | EFER_LME
    wrmsr

    ; 加载临时 GDT（包含 32 位和 64 位段）
    lea eax, [rel gdt_descriptor]
    lgdt [eax]

    ; 设置 CR3 指向 PML4（使用物理地址）
    lea eax, [rel pml4_table]
    mov cr3, eax

    ; 启用分页
    mov eax, cr0
    or eax, CR0_PG
    mov cr0, eax

    ; 现在已启用分页，执行 far jmp 进入 64 位代码
    ; GDT 段选择子 8（64位代码段）
    lea eax, [rel .long_mode_entry]
    push 8                      ; 64 位代码段选择子
    push eax
    retf                        ; far return = far jmp

    ; 以下是 32 位代码，在启用分页后不会执行到
    ; （保留用于调试或备用）

.already_long_mode:
    ; CPU 已经在 64 位长模式
    ; 这通常不会发生
    ; 如果发生了，显示错误并挂起

    ; 显示错误信息（使用 VGA 文本模式）
    mov byte [0xB8000], 'E'
    mov byte [0xB8001], 0x0F        ; 白底黑字
    mov byte [0xB8002], 'R'
    mov byte [0xB8003], 0x0F
    mov byte [0xB8004], 'R'
    mov byte [0xB8005], 0x0F

.already_long_mode_hang:
    hlt
    jmp .already_long_mode_hang

; ============================================================
; 64 位启动段
; ============================================================

section .boot
bits 64

.long_mode_64_entry:
    ; 进入 64 位模式的入口（来自 .already_long_mode 分支或 32位 far jmp）

    ; 设置数据段（64位数据段选择子 = 0x20）
    mov ax, 0x20
    mov ss, ax
    mov ds, ax
    mov es, ax
    mov fs, ax
    mov gs, ax

    ; 跳到公共 64 位设置
    jmp .long_mode_setup

.long_mode_entry:
    ; 进入 64 位模式的入口（来自 32 位 far jmp）

    ; 设置数据段（GDT 段选择子 16）
    mov ax, 16
    mov ss, ax
    mov ds, ax
    mov es, ax
    mov fs, ax
    mov gs, ax

.long_mode_setup:
    ; 统一的 64 位公共设置（无论从 32 位还是 64 位进来都会执行）

    ; ====================================================
    ; 第三步：完成分页和栈设置
    ; ====================================================

    ; 刷新 TLB 以应用新的页表项
    mov rax, cr3
    mov cr3, rax

    ; 设置栈指针
    ; 由于低地址恒等映射，栈既在物理地址 boot_stack_top 处
    ; 也在高地址 KERNEL_VIRT_BASE + boot_stack_top 处（通过页表映射）
    ;
    ; 选项：使用高地址栈（更符合高半核设计）
    ; 计算：高地址 = KERNEL_VIRT_BASE + (boot_stack_top 的物理偏移)
    ;
    ; 由于 boot_stack_top 在物理 ~0x100000 + offset，且恒等映射到低地址
    ; 同时也映射到高地址，我们使用高地址来设置栈

    lea rax, [rel boot_stack_top]    ; 物理地址（由于 RIP-relative）
    add rax, KERNEL_VIRT_BASE       ; 转换为高地址
    mov rsp, rax

    ; 清零 RBP（帮助调试器识别栈帧结束）
    xor rbp, rbp

    ; ====================================================
    ; 第四步：跳转到 Rust 入口
    ; ====================================================

    ; 使用 lea + jmp 跳转（避免 RIP 寻址问题）
    lea rax, [rel _boot_rust]
    call rax

    ; 如果 _boot_rust 返回（不应该），挂起
.hang:
    hlt
    jmp .hang

; ============================================================
; 页表初始化（物理地址区域）
; ============================================================
; 建立临时页表结构，映射：
; 1. [0x00000000, 0x100000000) → 物理 [0, 4GB)（恒等映射）
; 2. [0xFFFFFFFF80000000, 0xFFFFFFFFFFFFFFFF) → 物理 [0, 2GB)
;
; 页表必须放在物理低地址（.boot 段），这样在启用分页前可以访问

section .boot
align PAGE_SIZE

; 顶级页表（PML4）
; 512 个条目，每个条目 8 字节
pml4_table:
    alignb PAGE_SIZE
    resq PML4_SIZE

; 低地址 PDPT（映射 0-4GB）
pdpt_low:
    alignb PAGE_SIZE
    resq PDPT_SIZE

; 高地址 PDPT（映射高地址空间）
pdpt_high:
    alignb PAGE_SIZE
    resq PDPT_SIZE

; 低地址页目录表（足以映射 4GB）
; 注意：这里使用 2MB 巨页，2048 个条目映射 4GB
pd_low:
    alignb PAGE_SIZE
    resq 2048           ; 2048 * 2MB = 4GB

; 高地址页目录表
; 1024 个条目映射 2GB
pd_high:
    alignb PAGE_SIZE
    resq 1024           ; 1024 * 2MB = 2GB

; ============================================================
; 页表初始化代码
; ============================================================

section .boot
bits 32

; 初始化函数（在启用分页前调用）
; 此函数在 32 位 boot 代码中调用，然后才启用分页
; 但我们在 _boot 中直接初始化，不需要单独函数

; 页表初始化逻辑（内联在 _boot 中）：
; 注意：这些初始化发生在启用分页之前

; 初始化 PML4
; PML4[0] 指向 pdpt_low
; PML4[511] 指向 pdpt_high

; 初始化 pdpt_low
; pdpt_low[0..4] 分别指向 pd_low[0..4]

; 初始化 pdpt_high
; pdpt_high[510] 指向 pd_high

; 初始化 pd_low[0..4]
; 使用 2MB 巨页直接映射物理内存

; 初始化 pd_high
; 使用 2MB 巨页映射物理 [0, 2GB)

; 由于这些初始化不能在 NASM 中轻松以循环形式内联完成，
; 我们将使用一个辅助初始化函数

; ============================================================
; 页表初始化函数（在进入长模式前调用）
; ============================================================

bits 32

init_page_tables:
    ; 保存寄存器
    push ebp
    mov ebp, esp
    push ebx
    push esi
    push edi

    ; ====================================================
    ; 初始化 PML4
    ; ====================================================

    ; PML4[0] = pdpt_low | PTE_KERNEL
    lea eax, [rel pml4_table]
    lea ecx, [rel pdpt_low]
    or ecx, PTE_KERNEL
    mov dword [eax + 0], ecx        ; PML4[0] = pdpt_low
    mov dword [eax + 4], 0          ; 高 32 位

    ; PML4[511] = pdpt_high | PTE_KERNEL
    ; (因为 0xFFFFFFFF80000000 >> 39 & 0x1FF = 511)
    lea ecx, [rel pdpt_high]
    or ecx, PTE_KERNEL
    mov dword [eax + 511*8], ecx    ; PML4[511] = pdpt_high（低32位）
    mov dword [eax + 511*8 + 4], 0  ; 高 32 位

    ; ====================================================
    ; 初始化 pdpt_low（4 个条目，每个指向一个 PD，映射 1GB）
    ; ====================================================

    lea eax, [rel pdpt_low]
    lea ebx, [rel pd_low]

    mov ecx, 0
.init_pdpt_low_loop:
    cmp ecx, 4                      ; 4 个条目映射 4GB
    je .init_pdpt_low_done

    mov edx, ebx
    add edx, ecx
    shl edx, 12                     ; edx = pd_low + ecx * 4096（每个 PD）
    or edx, PTE_KERNEL

    mov esi, eax
    add esi, ecx
    shl esi, 3                      ; pdpt_low + ecx * 8

    mov [esi], edx                  ; pdpt_low[ecx] = pd_low[ecx]
    mov dword [esi + 4], 0

    inc ecx
    jmp .init_pdpt_low_loop

.init_pdpt_low_done:

    ; ====================================================
    ; 初始化 pdpt_high（1 个条目指向 pd_high）
    ; ====================================================

    lea eax, [rel pdpt_high]
    lea ecx, [rel pd_high]
    or ecx, PTE_KERNEL

    ; pdpt_high[510] = pd_high | PTE_KERNEL
    mov [eax + 510*8], ecx
    mov dword [eax + 510*8 + 4], 0

    ; ====================================================
    ; 初始化 pd_low（2MB 巨页，映射物理 0-4GB）
    ; ====================================================

    lea eax, [rel pd_low]

    ; 需要 2048 个 2MB 页项来映射 4GB
    mov ecx, 0                      ; 页号（0 to 2047）
    mov edx, 0                      ; 物理地址

.init_pd_low_loop:
    cmp ecx, 2048                   ; 2048 * 2MB = 4GB
    je .init_pd_low_done

    ; 创建 2MB 巨页表项
    mov ebx, edx
    or ebx, (PTE_KERNEL | PTE_HUGE)
    mov [eax + ecx*8], ebx          ; 低 32 位
    mov dword [eax + ecx*8 + 4], 0  ; 高 32 位

    add edx, (2 * 1024 * 1024)      ; 下一个 2MB
    inc ecx
    jmp .init_pd_low_loop

.init_pd_low_done:

    ; ====================================================
    ; 初始化 pd_high（2MB 巨页，映射物理 0-2GB）
    ; ====================================================

    lea eax, [rel pd_high]

    mov ecx, 0                      ; 页号
    mov edx, 0                      ; 物理地址

.init_pd_high_loop:
    cmp ecx, 1024                   ; 1024 * 2MB = 2GB
    je .init_pd_high_done

    mov ebx, edx
    or ebx, (PTE_KERNEL | PTE_HUGE)
    mov [eax + ecx*8], ebx          ; 低 32 位
    mov dword [eax + ecx*8 + 4], 0  ; 高 32 位

    add edx, (2 * 1024 * 1024)      ; 下一个 2MB
    inc ecx
    jmp .init_pd_high_loop

.init_pd_high_done:

    ; 恢复寄存器
    pop edi
    pop esi
    pop ebx
    pop ebp
    ret

; ============================================================
; GDT（全局描述符表）
; ============================================================

section .boot
align 8

gdt_table:
    ; GDT[0] - 空段描述符
    dq 0x0000000000000000

    ; GDT[1] - 32 位代码段
    ; 基地址: 0, 限制: 0xFFFFFFFF, DPL: 0, 类型: 代码（可执行、可读）
    dq 0x00CF9A000000FFFF

    ; GDT[2] - 32 位数据段
    dq 0x00CF92000000FFFF

    ; GDT[3] - 64 位代码段
    ; 长模式下，"粒度"和"操作数大小"标志被重解释
    dq 0x00A09A0000000000

    ; GDT[4] - 64 位数据段
    dq 0x00A0920000000000

gdt_descriptor:
    dw gdt_descriptor - gdt_table - 1   ; GDT 大小 - 1
    dq gdt_table                         ; GDT 基地址

; ============================================================
; 引导栈（物理低地址）
; ============================================================
; 栈必须在物理低地址（.boot 段），这样在启用分页前后都能访问

section .boot
align 16

boot_stack_bottom:
    resb 16384
boot_stack_top:

; ============================================================
; Multiboot2 头（用于 GRUB2）
; ============================================================

section .multiboot2
align 8

multiboot_header:
    dd 0xE85250D6               ; 魔数
    dd 0                        ; 架构（0 = i386）
    dd multiboot_header_end - multiboot_header ; 头长度
    dd -(0xE85250D6 + 0 + (multiboot_header_end - multiboot_header))

    ; 标签 0（结束标记）
    dw 0
    dw 0
    dd 8

multiboot_header_end:
