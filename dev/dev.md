# AXIS 开发者文档

[项目概述](#项目概述) | [如何开始](#如何开始) | [协作规范](#协作规范) | [开发计划](#开发计划) | [附录](#附录)

<br>



## 项目概述

### 简介

**AXIS** — **A**XIS e**X**ecute **I**nstructions **S**teadily.

AXIS 是一个用 Rust 编写的宏内核，稳定、直接、忠实地执行每一条指令。我们坚持 Unix 风格的内核设计，以稳定可靠地执行硬件指令为核心目标，在统一内核地址空间内完成进程调度、内存管理、中断处理、设备驱动、文件系统等功能，追求简洁的内核架构与可预测的执行行为。AXIS 面向底层操作系统开发实践，探索宏内核的设计思路，专注于夯实基础执行能力，为上层软件提供稳定的运行底座。

*我们编写 AXIS，作为同济大学计算机科学与技术学院软件工程专业 2026 年暑期《操作系统课程设计》的一个课程项目。*

### 要求

- 完整的类 Unix 宏内核架构
- 模块化、可维护的代码结构
- 高复用率、低冗余、风格规整统一的高质量代码
- 采用 Linux 定义的通用系统调用 ABI 和语义
- 不涉及平台特定的寄存器映射由 `arch/` 层完全处理
- 后期将支持 musl 动态链接的程序运行，为此：
   - 须实现 fork, execve, exit, read, write, mmap 等核心系统调用
   - 须正确处理 errno（内核返回负错误码，libc 转换为 errno）
   - 须采用标准的程序加载格式：ELF，正确的 argv/envp/auxv 栈布局

### 架构设计

详见 [架构设计文档](arch.md)

### 参考资料

[Philipp Oppermann's blog](https://os.phil-opp.com/zh-CN/)

<br>



## 如何开始

...（待完善）

<br>



## 协作规范

1. Commit 消息遵循 *Conventional Commits* 规范，格式如下：

    ```txt
    [<type>(<scope>)] <subject>
    ```

    **使用英文中括号，用简体中文简要描述更改，并在中英文字符间添加空格**。例：

    ```txt
    [feat(vfs)] 完成虚拟文件系统主要功能，以便接入各种文件系统
    ```

    注意，在 `]` 和 `完` 之间有一个空格。更多相关规范，详见[附录](#conventional-commits)。

2. 不要对 `main` 直接操作，而是创建功能性 branch，在其上进行开发  
    **分支命名格式为 `author/module`**，其中 `author` 为开发者姓名简拼。例：

    ```txt
    qhz/bootloader
    ```

3. 代码**注释使用中文**，后续扩展至界面语言、提示语言和报错信息等均使用中文。
   注释**不仅要写清楚正在做什么，更要讲明白为什么要这样做**

4. 命名约定
   - 模块/文件名：snake_case（小写加下划线）
   - 类型/结构：PascalCase
   - 常量：UPPER_CASE
   - 函数/方法：snake_case
   - 私有项前缀：_ 或放在私有模块中

4. 对于引入的任何概念，都力求搞清摸透能解释（宁可少做一点）

5. 对于每个模块，可以遵循「先手搓助理解，再上库求性能」的开发路线

6. 尽量选用高效的算法，反正是 Agent 写不是人写（误）

<br>



## 开发计划

详见 [落地路线计划文档](roadmap.md)

<br>



## 附录

### Conventional Commits 

#### 一、核心类型速查表

| 类型 | 说明 | 典例 |
|---|---|---|
| **feat** | 新功能（Feature） | `[feat(auth)] 新增手机号登录` |
| **fix** | 修复 Bug | `[fix(order)] 解决支付超时未回调问题` |
| **docs** | 文档变更（README/注释） | `[docs] 更新部署文档` |
| **style** | 代码格式（空格/缩进/分号，无逻辑变更） | `[style] 格式化代码，删除多余空行` |
| **refactor** | 代码重构（非功能新增/修复） | `[refactor(user)] 重构用户信息查询逻辑` |
| **perf** | 性能优化 | `[perf(list)] 优化列表渲染，减少重渲染` |
| **test** | 新增/修改测试用例 | `[test(cart)] 新增购物车结算单元测试` |
| **chore** | 构建/工具/依赖调整（无业务代码变更） | `[chore] 升级依赖包版本` |
| **ci** | CI/CD 配置变更 | `[ci] 配置 GitHub Actions 自动部署` |
| **build** | 构建系统/外部依赖变更 | `[build] 调整 Webpack 打包配置` |
| **revert** | 撤销之前的 Commit | `[revert] 回滚“feat: 新增优惠券”提交` |

#### 二、标准格式

```
[<type>(<scope>)] <subject>
```

- **type**：必填，上表核心类型之一。
- **scope**：可选，影响范围（模块/文件/功能，如 `auth`、`order`）。
- **subject**：必填，简短描述（≤50字符，简体中文祈使句、无句号）。

#### 三、实用场景分类与关键词

1. **功能新增（feat）**
   - 关键词：`add`、`implement`、`support`、`introduce`。
   - 示例：`[feat(pay)] 新增支付宝支付渠道`。

2. **问题修复（fix）**
   - 关键词：`resolve`、`fix`、`patch`、`address`。
   - 示例：`[fix(ui)] 修复移动端按钮错位问题`。

3. **代码优化（refactor/perf）**
   - 关键词：`optimize`、`improve`、`simplify`、`reduce`。
   - 示例：`[refactor(common)] 提取公共工具类`；`[perf(api)] 优化数据库查询，减少响应时间`。

4. **文档与格式（docs/style）**
   - 示例：`[docs(api)] 更新接口参数说明`；`[style] 统一代码缩进为 4 空格`。

5. **测试与工具（test/chore）**
   - 示例：`[test(unit)] 新增用户注册逻辑测试`；`[chore] 配置 ESLint 代码检查`。
