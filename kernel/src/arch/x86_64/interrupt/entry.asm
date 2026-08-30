; ============================================================
; x86_64 中断入口存根
; ============================================================
; 保存/恢复寄存器，调用 Rust 中断处理函数
;
; Intel 语法，64 位模式

section .text

; 为什么需要汇编存根：
; 1. CPU 自动压栈的内容有限（RIP、CS、RFLAGS、RSP、SS）
; 2. 需要手动保存所有通用寄存器，以便中断处理函数可以使用
; 3. 需要处理错误码（某些异常 CPU 会压入错误码，某些不会）
; 4. 需要确保栈 16 字节对齐（x86_64 ABI 要求）

; 寄存器保存布局（从高地址到低地址）：
; +--------+
; | SS     | <-- CPU 压栈（特权级切换时）
; | RSP    |
; | RFLAGS |
; | CS     |
; | RIP    |
; | 错误码  | <-- 部分异常有，部分没有
; +--------+
; | R15    | <-- 以下是我们手动保存的
; | R14    |
; | R13    |
; | R12    |
; | R11    |
; | R10    |
; | R9     |
; | R8     |
; | RBP    |
; | RDI    |
; | RSI    |
; | RDX    |
; | RCX    |
; | RBX    |
; | RAX    |
; +--------+

; 宏：定义不带错误码的异常处理程序
%macro EXCEPTION_NO_ERROR 1
global exception_%1_handler
exception_%1_handler:
    ; 压入伪错误码（保持栈布局一致）
    push 0
    ; 压入异常向量号
    push %1
    jmp exception_common
%endmacro

; 宏：定义带错误码的异常处理程序
%macro EXCEPTION_WITH_ERROR 1
global exception_%1_handler
exception_%1_handler:
    ; CPU 已经压入了错误码
    ; 压入异常向量号
    push %1
    jmp exception_common
%endmacro

; CPU 异常处理程序入口
EXCEPTION_NO_ERROR 0    ; #DE 除零错误
EXCEPTION_NO_ERROR 1    ; #DB 调试
EXCEPTION_NO_ERROR 2    ; NMI
EXCEPTION_NO_ERROR 3    ; #BP 断点
EXCEPTION_NO_ERROR 4    ; #OF 溢出
EXCEPTION_NO_ERROR 5    ; #BR 越界
EXCEPTION_NO_ERROR 6    ; #UD 无效操作码
EXCEPTION_NO_ERROR 7    ; #NM 设备不可用
EXCEPTION_WITH_ERROR 8  ; #DF 双重故障
EXCEPTION_WITH_ERROR 10 ; #TS 无效 TSS
EXCEPTION_WITH_ERROR 11 ; #NP 段不存在
EXCEPTION_WITH_ERROR 12 ; #SS 栈段错误
EXCEPTION_WITH_ERROR 13 ; #GP 一般保护错误
EXCEPTION_WITH_ERROR 14 ; #PF 页错误
EXCEPTION_NO_ERROR 16   ; #MF x87 浮点异常
EXCEPTION_WITH_ERROR 17 ; #AC 对齐检查
EXCEPTION_NO_ERROR 18   ; #MC 机器检查
EXCEPTION_NO_ERROR 19   ; #XM SIMD 浮点异常
EXCEPTION_NO_ERROR 20   ; #VE 虚拟化异常

; 异常处理公共路径
exception_common:
    ; 保存所有通用寄存器
    push rax
    push rbx
    push rcx
    push rdx
    push rsi
    push rdi
    push rbp
    push r8
    push r9
    push r10
    push r11
    push r12
    push r13
    push r14
    push r15

    ; 调用 Rust 处理函数
    ; 参数 1 (RDI): 异常向量号
    ; 参数 2 (RSI): 错误码
    ; 参数 3 (RDX): 中断帧指针
    mov rdi, [rsp + 15*8]       ; 异常向量号
    mov rsi, [rsp + 16*8]       ; 错误码
    lea rdx, [rsp + 17*8]       ; 中断帧地址

    ; 调用 Rust 函数
    extern handle_exception
    call handle_exception

    ; 恢复所有通用寄存器
    pop r15
    pop r14
    pop r13
    pop r12
    pop r11
    pop r10
    pop r9
    pop r8
    pop rbp
    pop rdi
    pop rsi
    pop rdx
    pop rcx
    pop rbx
    pop rax

    ; 弹出异常向量号和错误码
    add rsp, 16

    ; 中断返回
    ; IRETQ 会：
    ; 1. 弹出 RIP、CS、RFLAGS
    ; 2. 如果有特权级切换，弹出 RSP、SS
    ; 3. 恢复执行
    iretq

; 宏：定义硬件中断处理程序
%macro IRQ_HANDLER 1
global irq_%1_handler
irq_%1_handler:
    push 0                      ; 伪错误码
    push (32 + %1)              ; 中断向量号（IRQ 从 32 开始）
    jmp irq_common
%endmacro

; 硬件中断处理程序入口（IRQ 0-15）
IRQ_HANDLER 0
IRQ_HANDLER 1
IRQ_HANDLER 2
IRQ_HANDLER 3
IRQ_HANDLER 4
IRQ_HANDLER 5
IRQ_HANDLER 6
IRQ_HANDLER 7
IRQ_HANDLER 8
IRQ_HANDLER 9
IRQ_HANDLER 10
IRQ_HANDLER 11
IRQ_HANDLER 12
IRQ_HANDLER 13
IRQ_HANDLER 14
IRQ_HANDLER 15

; 硬件中断公共路径
irq_common:
    ; 保存所有通用寄存器
    push rax
    push rbx
    push rcx
    push rdx
    push rsi
    push rdi
    push rbp
    push r8
    push r9
    push r10
    push r11
    push r12
    push r13
    push r14
    push r15

    ; 调用 Rust 处理函数
    mov rdi, [rsp + 15*8]       ; 中断向量号
    lea rsi, [rsp + 17*8]       ; 中断帧地址

    extern handle_irq
    call handle_irq

    ; handle_irq 返回新的栈指针（0 = 不切换）：
    ; 调度器决定切换时，返回目标任务的保存帧 RSP；
    ; 切换到新栈后，下面的弹栈序列从新任务的帧恢复寄存器，
    ; iretq 落入新任务上下文——这就是"中断返回式上下文切换"
    test rax, rax
    jz .no_switch
    mov rsp, rax
.no_switch:

    ; 恢复所有通用寄存器
    pop r15
    pop r14
    pop r13
    pop r12
    pop r11
    pop r10
    pop r9
    pop r8
    pop rbp
    pop rdi
    pop rsi
    pop rdx
    pop rcx
    pop rbx
    pop rax

    ; 弹出中断向量号和伪错误码
    add rsp, 16

    ; 中断返回
    iretq
