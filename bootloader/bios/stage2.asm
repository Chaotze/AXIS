; ============================================================
; BIOS Stage2 引导加载程序（保护模式到长模式）
; ============================================================
; 这是 BIOS 引导模式下的第二阶段引导代码
; 功能：
;   1. 验证 CPU 支持长模式（64 位）
;   2. 启用 PAE（物理地址扩展）
;   3. 建立临时 4 级页表
;   4. 启用分页和长模式
;   5. 跳转到 Rust Stage2
;
; 入口：32 位保护模式，CS=0x08, DS/ES/SS=0x10
; 出口：64 位长模式，跳转到 stage2_rust
; ============================================================

[bits 32]               ; 32 位保护模式

; ---------- 常量 ----------
VGA_MEM          equ 0xB8000
CR0_PG           equ 1 << 31
CR4_PAE          equ 1 << 5
EFER_LME         equ 1 << 8

; ---------- 入口点 ----------
global stage2_entry
stage2_entry:
    ; 清屏
    mov edi, VGA_MEM
    mov ecx, 80 * 25
    mov ax, 0x0F20          ; 空格字符 + 属性
    rep stosw

    ; 关闭 VGA 硬件光标
    ; 1. 选择光标起始寄存器 (索引 0x0A)
    mov dx, 0x3D4          ; 将索引端口地址放入 dx
    mov al, 0x0A           ; 要操作的寄存器索引
    out dx, al             ; 写入索引端口，选中寄存器 0x0A

    ; 2. 向光标起始寄存器写入值 0x20
    mov dx, 0x3D5          ; 将数据端口地址放入 dx
    mov al, 0x20           ; 一个典型的“关闭”值，通常用于让光标不可见
    out dx, al             ; 将 0x20 写入光标起始寄存器

    ; 3. 选择光标结束寄存器 (索引 0x0B)
    mov dx, 0x3D4
    mov al, 0x0B           ; 选择索引 0x0B
    out dx, al
    popa

    ; 检查 CPUID
    call check_cpuid
    test eax, eax
    jz .no_cpuid

    ; 检查长模式
    call check_long_mode
    test eax, eax
    jz .no_long_mode

    ; 设置页表
    call setup_page_tables

    ; 启用 PAE 和长模式
    call enable_paging

    ; 加载 64 位 GDT
    lgdt [gdt64_descriptor]

    ; 跳转到 64 位代码
    jmp 0x08:long_mode_entry

.no_cpuid:
    mov esi, msg_no_cpuid
    call print_string_32
    jmp $

.no_long_mode:
    mov esi, msg_no_long_mode
    call print_string_32
    jmp $

; ------------------------------------------------------------
; 检查 CPU 是否支持 CPUID 指令
; ------------------------------------------------------------
; CPUID 指令是获取 CPU 信息的标准接口
; 老旧的 CPU（如 486 之前）不支持 CPUID
;
; 检测方法：尝试翻转 EFLAGS 的 ID 位（位 21）
;   - 如果 ID 位可以被修改，说明支持 CPUID
;   - 如果 ID 位无法修改，说明不支持 CPUID
;
; 返回：EAX = 1（支持）或 0（不支持）
check_cpuid:
    pushfd                  ; 保存 EFLAGS
    pop eax
    mov ecx, eax            ; 保存原始值

    ; 翻转 ID 位（位 21）
    xor eax, 1 << 21
    push eax
    popfd                   ; 写回 EFLAGS

    ; 读取 EFLAGS，检查 ID 位是否被修改
    pushfd
    pop eax
    xor eax, ecx            ; 比较修改前后的值

    ; 恢复原始 EFLAGS
    push ecx
    popfd

    ; 如果 ID 位被修改，EAX 的位 21 为 1
    and eax, 1 << 21
    ret

; ------------------------------------------------------------
; 检查 CPU 是否支持长模式（64 位）
; ------------------------------------------------------------
; 使用 CPUID 扩展功能检测
;
; 检测步骤：
;   1. CPUID EAX=0x80000000：查询最大扩展功能号
;   2. 如果最大功能号 >= 0x80000001，则支持扩展功能
;   3. CPUID EAX=0x80000001，EDX 的位 29 表示是否支持长模式
;
; 返回：EAX = 1（支持）或 0（不支持）
check_long_mode:
    ; 查询 CPUID 最大扩展功能号
    mov eax, 0x80000000
    cpuid
    cmp eax, 0x80000001     ; 是否支持 0x80000001 功能？
    jb .no_support

    ; 查询扩展处理器信息和特性
    mov eax, 0x80000001
    cpuid
    test edx, 1 << 29       ; 位 29 = 长模式支持位
    jz .no_support

    mov eax, 1              ; 支持
    ret

.no_support:
    xor eax, eax            ; 不支持
    ret

; ------------------------------------------------------------
; 建立临时 4 级页表（标识映射低 2GB）
; ------------------------------------------------------------
; 长模式（64 位）使用 4 级页表：
;   - PML4（Page Map Level 4）：512 个条目，每个覆盖 512GB
;   - PDPT（Page Directory Pointer Table）：512 个条目，每个覆盖 1GB
;   - PD（Page Directory）：512 个条目，每个覆盖 2MB
;   - PT（Page Table）：512 个条目，每个覆盖 4KB
;
; 为了简化，我们使用 2MB 大页直接映射低 2GB 内存：
;   - PML4[0] -> PDPT
;   - PDPT[0] -> PD
;   - PD[0..1023] -> 2MB 物理页（标识映射）
;
; 标识映射（Identity Mapping）：虚拟地址 = 物理地址
; 为什么使用标识映射？
;   - 引导代码运行在低物理地址（如 0x7e00）
;   - 启用分页后，如果虚拟地址不同，会立即崩溃
;   - 标识映射确保当前运行的代码在启用分页后仍然有效
setup_page_tables:
    ; 清零页表内存（从 0x1000 开始，使用 16KB）
    ; 为什么从 0x1000 开始？
    ;   - 0x0000-0x0FFF：中断向量表和 BIOS 数据区
    ;   - 0x7c00-0x7dff：MBR（Stage1）
    ;   - 0x7e00-0x9fff：Stage2 代码和数据
    ;   - 0x1000-0x4fff：页表区域（PML4、PDPT、PD）
    mov edi, 0x1000
    mov ecx, 0x4000 / 4     ; 16KB / 4 字节 = 4096 个 DWORD
    xor eax, eax
    rep stosd

    ; 设置 PML4[0] -> PDPT（0x2000）
    mov edi, 0x1000         ; PML4 基地址
    mov dword [edi], 0x2003 ; PDPT 地址 | P=1, RW=1
    ; 0x2003 = 0x2000（PDPT 地址）| 0x3（P=1, RW=1）
    ; P（Present）= 1：页表存在
    ; RW（Read/Write）= 1：可读写

    ; 设置 PDPT[0] -> PD（0x3000）
    mov edi, 0x2000         ; PDPT 基地址
    mov dword [edi], 0x3003 ; PD 地址 | P=1, RW=1

    ; 设置 PD[0..1023]：映射 1024 个 2MB 大页（共 2GB）
    ; 为什么映射 2GB？
    ;   - 覆盖低内存区域，足够引导使用
    ;   - 内核后续会建立完整的虚拟内存映射
    mov edi, 0x3000         ; PD 基地址
    mov eax, 0x83           ; 第一个 2MB 页：0x000000 | P=1, RW=1, PS=1
    ; PS（Page Size）= 1：2MB 大页（否则需要 PT）
    mov ecx, 1024           ; 1024 个条目
.loop:
    mov [edi], eax
    add eax, 0x200000       ; 下一个 2MB 页
    add edi, 8              ; 下一个条目（64 位）
    loop .loop

    ret

; ------------------------------------------------------------
; 启用 PAE 和分页，进入长模式
; ------------------------------------------------------------
; 步骤：
;   1. 启用 PAE（Physical Address Extension）
;   2. 加载 PML4 基地址到 CR3
;   3. 启用长模式（设置 EFER.LME）
;   4. 启用分页（设置 CR0.PG）
;
; 为什么需要 PAE？
;   - 长模式强制要求 PAE
;   - PAE 扩展页表项从 32 位到 64 位，支持 64 位物理地址
enable_paging:
    ; 启用 PAE（CR4.PAE = 1）
    mov eax, cr4
    or eax, CR4_PAE         ; 设置位 5（PAE 位）
    mov cr4, eax

    ; 加载 PML4 基地址到 CR3
    ; CR3 是页表基址寄存器
    mov eax, 0x1000         ; PML4 地址
    mov cr3, eax

    ; 启用长模式（EFER.LME = 1）
    ; EFER（Extended Feature Enable Register）是 MSR 寄存器
    ; MSR 通过 RDMSR/WRMSR 指令访问
    mov ecx, 0xC0000080     ; EFER 的 MSR 地址
    rdmsr                   ; 读取到 EDX:EAX
    or eax, EFER_LME        ; 设置位 8（LME = Long Mode Enable）
    wrmsr                   ; 写回

    ; 启用分页（CR0.PG = 1）
    ; 设置 PG 位后，CPU 立即开始使用页表进行地址转换
    ; 同时，LME 位变为 LMA（Long Mode Active），CPU 进入长模式
    mov eax, cr0
    or eax, CR0_PG          ; 设置位 31（PG 位）
    mov cr0, eax

    ret

; ------------------------------------------------------------
; 32 位打印函数
; ------------------------------------------------------------
; VGA 文本模式内存映射：0xB8000
; 每个字符占用 2 字节：[字符, 属性]
; 属性字节：[闪烁, 背景色(3), 高亮, 前景色(3)]
;
; ESI：字符串地址（以 null 结尾）
print_string_32:
    pusha
    mov edi, VGA_MEM        ; VGA 文本缓冲区
    mov ah, 0               ; 默认：白色文本黑色背景 (等同 0x07)
.loop:
    lodsb                   ; AL = [ESI], ESI++
    test al, al             ; 检查是否为 null
    jz .done
    stosw
    jmp .loop
.done:
    ; 串口发送换行（可选，这里不自动加）
    popa
    ret

; ------------------------------------------------------------
; 64 位长模式入口
; ------------------------------------------------------------
[bits 64]
long_mode_entry:
    ; 清空段寄存器（长模式下不使用段）
    ; 为什么清空？
    ;   - 长模式使用平坦内存模型，不需要段
    ;   - 清空避免旧的段选择子导致问题
    xor ax, ax
    mov ds, ax
    mov es, ax
    mov fs, ax
    mov gs, ax
    mov ss, ax

    ; 设置 64 位堆栈
    mov rsp, 0x200000       ; 使用 2MB 地址作为堆栈基址

    ; 调用 Rust Stage2
    ; Rust 代码编译为 extern "C" 函数，遵循 System V AMD64 ABI
    extern stage2_rust
    call stage2_rust

    ; 如果返回，挂起
    jmp $

; ------------------------------------------------------------
; 64 位 GDT
; ------------------------------------------------------------
; 长模式下，段描述符的大部分字段被忽略
; 但仍需要有效的 GDT，主要用于设置代码段的 L 位（64 位模式）
align 8
gdt64_start:
    ; 空描述符
    dq 0

gdt64_code:
    ; 64 位代码段
    ; L=1（64 位模式）, P=1（存在）, DPL=0（内核权限）
    dq (1 << 43) | (1 << 44) | (1 << 47) | (1 << 53)
    ; 位 43: 可执行
    ; 位 44: 代码/数据段（非系统段）
    ; 位 47: 存在
    ; 位 53: 64 位模式

gdt64_data:
    ; 64 位数据段（实际不使用，但保留以兼容）
    dq (1 << 44) | (1 << 47) | (1 << 41)
    ; 位 41: 可写
    ; 位 44: 代码/数据段
    ; 位 47: 存在

gdt64_end:

gdt64_descriptor:
    dw gdt64_end - gdt64_start - 1  ; 界限
    dq gdt64_start                  ; 基地址（64 位）

; ------------------------------------------------------------
; 数据区
; ------------------------------------------------------------
msg_start:              db "Stage2: Starting...", 0
msg_cpuid_ok:           db "  CPUID supported.", 0
msg_long_mode_ok:       db "  Long mode supported.", 0
msg_no_cpuid:           db "ERROR: CPUID not supported", 0
msg_no_long_mode:       db "ERROR: Long mode not supported", 0
msg_cr0_before:         db "  CR0 before: 0x", 0
msg_cr4_before:         db "  CR4 before: 0x", 0
msg_setup_pagetables:   db "  Setting up page tables...", 0
msg_pagetables_done:    db "  Page tables set up.", 0
msg_enable_paging:      db "  Enabling paging and long mode...", 0
msg_paging_done:        db "  Paging enabled, long mode active.", 0
msg_cr0_after:          db "  CR0 after:  0x", 0
msg_cr4_after:          db "  CR4 after:  0x", 0
msg_efer_after:         db "  EFER after: 0x", 0
msg_load_gdt:           db "  Loading 64-bit GDT...", 0
msg_gdt_loaded:         db "  GDT loaded.", 0
msg_jump_long:          db "  Jumping to 64-bit code...", 0
msg_long_mode_enter:    db "Long Mode: Entered successfully!", 0
msg_rsp:                db "  RSP = 0x", 0
msg_call_rust:          db "  Calling Rust stage2...", 0
msg_returned:           db "  Rust returned (should not happen), halting.", 0
newline:                db 0x0D, 0x0A, 0   ; CR + LF
