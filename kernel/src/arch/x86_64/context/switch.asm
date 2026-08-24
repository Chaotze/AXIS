; ============================================================
; x86_64 上下文切换
; ============================================================
; 保存旧任务状态，恢复新任务状态
;
; Intel 语法，64 位模式

section .text

; 函数原型：
; void context_switch(TrapFrame* old, const TrapFrame* new)
;
; 参数：
; - RDI: old（保存当前状态的位置）
; - RSI: new（要恢复的状态）

global context_switch
context_switch:
    ; 保存当前任务的寄存器到 old
    mov [rdi + 0x00], r15
    mov [rdi + 0x08], r14
    mov [rdi + 0x10], r13
    mov [rdi + 0x18], r12
    mov [rdi + 0x20], r11
    mov [rdi + 0x28], r10
    mov [rdi + 0x30], r9
    mov [rdi + 0x38], r8
    mov [rdi + 0x40], rbp
    ; RDI 和 RSI 暂时跳过（稍后保存）
    mov [rdi + 0x58], rdx
    mov [rdi + 0x60], rcx
    mov [rdi + 0x68], rbx
    mov [rdi + 0x70], rax

    ; 保存 RDI 和 RSI
    mov rax, rdi
    mov [rdi + 0x48], rax  ; 原始 RDI
    mov rax, rsi
    mov [rdi + 0x50], rax  ; 原始 RSI

    ; 保存栈指针和返回地址
    ; 注意：当前栈顶是返回地址
    mov rax, rsp
    add rax, 8             ; 跳过返回地址
    mov [rdi + 0xB8], rax  ; RSP

    mov rax, [rsp]         ; 返回地址
    mov [rdi + 0xA0], rax  ; RIP

    ; 保存 RFLAGS
    pushfq
    pop rax
    mov [rdi + 0xB0], rax  ; RFLAGS

    ; 保存段寄存器
    mov ax, cs
    mov [rdi + 0xA8], rax  ; CS
    mov ax, ss
    mov [rdi + 0xC0], rax  ; SS

    ; 恢复新任务的寄存器
    mov r15, [rsi + 0x00]
    mov r14, [rsi + 0x08]
    mov r13, [rsi + 0x10]
    mov r12, [rsi + 0x18]
    mov r11, [rsi + 0x20]
    mov r10, [rsi + 0x28]
    mov r9,  [rsi + 0x30]
    mov r8,  [rsi + 0x38]
    mov rbp, [rsi + 0x40]
    ; RDI 和 RSI 稍后恢复
    mov rdx, [rsi + 0x58]
    mov rcx, [rsi + 0x60]
    mov rbx, [rsi + 0x68]
    mov rax, [rsi + 0x70]

    ; 恢复栈指针
    mov rsp, [rsi + 0xB8]

    ; 压入新任务的返回地址
    push qword [rsi + 0xA0]  ; RIP

    ; 恢复 RDI 和 RSI
    mov rdi, [rsi + 0x48]
    ; RSI 最后恢复（因为还需要使用它）
    mov rax, [rsi + 0x50]
    mov rsi, rax

    ; 返回到新任务
    ret
