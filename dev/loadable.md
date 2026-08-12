# AXIS 可加载模块化改造文档

    注意，该方案实施日期待定，请暂时忽略。

[设计概览](#设计概览) | [核心组件](#核心组件) | [开发路线](#开发路线) | [挑战及解决方案](#挑战及解决方案) | [参考资料](#参考资料)

<br>



## 设计概览

仿照 Linux 内核模块机制，为 AXIS 内核实现驱动/文件系统/网络协议栈的内核模块动态加载能力。

```
┌─────────────────────────────────────────────────┐
│           AXIS Kernel Core (静态链接)            │
├─────────────────────────────────────────────────┤
│  - 内存管理 (mm/)                                │
│  - 任务调度 (task/)                              │
│  - 模块加载器 (module/)  ← 新增                  │
│  - 符号导出表 (exports/)  ← 新增                 │
└─────────────────────────────────────────────────┘
           ↑ ↑ ↑
           │ │ │ 运行时加载
           │ │ │
    ┌──────┘ │ └──────┐
    │        │        │
┌───▼───┐ ┌──▼──┐ ┌──▼───┐
│ e1000 │ │exfat│ │ tcp  │
│ .axm  │ │.axm │ │ .axm │  ← .axm = AXIS Module
└───────┘ └─────┘ └──────┘
```

<br>



## 核心组件

### 1. 模块文件格式 (.axm)

**.axm 文件 = 修改的 ELF 格式**

```
e1000.axm (ELF-based)
├── .text                    # 代码段
├── .data                    # 数据段
├── .rodata                  # 只读数据
├── .bss                     # 未初始化数据
├── .symtab                  # 符号表
├── .strtab                  # 字符串表
├── .axm_reloc               # AXIS 重定位表 ← 自定义
├── .axm_deps                # 依赖的内核符号 ← 自定义
├── .axm_meta                # 模块元数据 ← 自定义
└── .axm_init                # init/exit 函数指针 ← 自定义
```

**元数据结构**：
```rust
#[repr(C)]
pub struct AxmMetadata {
    pub magic: u32,              // 0x41584D00 ("AXM\0")
    pub version: u32,            // 模块版本
    pub kernel_version: u32,     // 编译时的内核版本
    pub name: [u8; 64],          // 模块名
    pub author: [u8; 128],
    pub license: [u8; 32],       // "GPL", "MIT" 等
    pub module_type: ModuleType, // Driver, FileSystem, NetProtocol
    pub flags: u32,              // 可选特性
}

#[repr(u32)]
pub enum ModuleType {
    Driver = 0,
    FileSystem = 1,
    NetProtocol = 2,
    Generic = 3,
}
```

---

### 2. 符号导出系统

**内核侧：导出符号**

```rust
// kernel/src/exports/mod.rs

use core::sync::atomic::{AtomicPtr, Ordering};

/// 符号导出宏
#[macro_export]
macro_rules! export_symbol {
    ($name:ident) => {
        #[used]
        #[link_section = ".kernel_exports"]
        static $name_EXPORT: $crate::exports::KernelSymbol = 
            $crate::exports::KernelSymbol {
                name: concat!(stringify!($name), "\0"),
                address: $name as *const () as usize,
                crc: $crate::exports::calculate_crc(stringify!($name)),
            };
    };
}

#[repr(C)]
pub struct KernelSymbol {
    pub name: &'static str,
    pub address: usize,
    pub crc: u32,  // 版本检查用
}

// 内核符号表（启动时构建）
pub struct SymbolTable {
    symbols: BTreeMap<&'static str, KernelSymbol>,
}

impl SymbolTable {
    pub fn lookup(&self, name: &str) -> Option<usize> {
        self.symbols.get(name).map(|sym| sym.address)
    }
    
    pub fn verify_crc(&self, name: &str, crc: u32) -> bool {
        self.symbols.get(name)
            .map(|sym| sym.crc == crc)
            .unwrap_or(false)
    }
}

// 全局符号表
pub static SYMBOL_TABLE: OnceCell<SymbolTable> = OnceCell::new();
```

**使用示例**：
```rust
// kernel/src/mm/pmm.rs

pub fn alloc_page() -> Option<PhysAddr> {
    // ... 实现
}

export_symbol!(alloc_page);  // 导出给模块使用
```

---

### 3. 模块加载器

**核心结构**：

```rust
// kernel/src/module/mod.rs

pub struct ModuleLoader {
    loaded_modules: SpinLock<BTreeMap<ModuleId, Arc<LoadedModule>>>,
    next_id: AtomicU32,
}

pub struct LoadedModule {
    pub id: ModuleId,
    pub name: String,
    pub state: ModuleState,
    pub metadata: AxmMetadata,
    
    // 内存布局
    pub code_base: VirtAddr,
    pub data_base: VirtAddr,
    pub code_size: usize,
    pub data_size: usize,
    
    // 初始化函数
    pub init_fn: Option<extern "C" fn() -> i32>,
    pub exit_fn: Option<extern "C" fn()>,
    
    // 符号表（导出的符号）
    pub exported_symbols: BTreeMap<String, usize>,
    
    // 依赖
    pub dependencies: Vec<ModuleId>,
    pub ref_count: AtomicU32,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum ModuleState {
    Loading,
    Live,
    Unloading,
    Dead,
}

pub type ModuleId = u32;
```

**加载流程**：

```rust
impl ModuleLoader {
    /// 加载模块（主入口）
    pub fn load_module(&self, module_data: &[u8]) -> Result<ModuleId, ModuleError> {
        // 1. 解析 ELF 头
        let elf = ElfParser::parse(module_data)?;
        
        // 2. 验证模块合法性
        self.verify_module(&elf)?;
        
        // 3. 分配内存
        let (code_base, data_base) = self.allocate_module_memory(&elf)?;
        
        // 4. 复制段到内存
        self.copy_sections(&elf, code_base, data_base)?;
        
        // 5. 解析依赖符号
        let dependencies = self.resolve_symbols(&elf)?;
        
        // 6. 应用重定位
        self.apply_relocations(&elf, code_base, &dependencies)?;
        
        // 7. 设置内存保护
        self.protect_memory(code_base, data_base)?;
        
        // 8. 创建模块对象
        let module = Arc::new(LoadedModule {
            id: self.next_id.fetch_add(1, Ordering::SeqCst),
            name: elf.get_module_name()?,
            state: ModuleState::Loading,
            metadata: elf.get_metadata()?,
            code_base,
            data_base,
            code_size: elf.code_size(),
            data_size: elf.data_size(),
            init_fn: elf.get_init_fn(),
            exit_fn: elf.get_exit_fn(),
            exported_symbols: elf.get_exports()?,
            dependencies: Vec::new(),
            ref_count: AtomicU32::new(0),
        });
        
        // 9. 注册模块
        self.loaded_modules.lock().insert(module.id, Arc::clone(&module));
        
        // 10. 调用 init 函数
        if let Some(init_fn) = module.init_fn {
            let ret = init_fn();
            if ret != 0 {
                self.unload_module(module.id)?;
                return Err(ModuleError::InitFailed(ret));
            }
        }
        
        // 11. 标记为 Live
        Arc::get_mut(&module).unwrap().state = ModuleState::Live;
        
        Ok(module.id)
    }
}
```

---

### 4. 符号解析器

```rust
// kernel/src/module/resolver.rs

pub struct SymbolResolver;

impl SymbolResolver {
    /// 解析模块需要的所有符号
    pub fn resolve_symbols(
        &self,
        elf: &ElfParser,
    ) -> Result<HashMap<String, usize>, ResolveError> {
        let mut resolved = HashMap::new();
        
        // 读取 .axm_deps 段
        let deps_section = elf.find_section(".axm_deps")?;
        let dependencies = self.parse_dependencies(deps_section)?;
        
        for dep in dependencies {
            // 从内核符号表查找
            let addr = SYMBOL_TABLE.get()
                .and_then(|table| table.lookup(&dep.name))
                .ok_or(ResolveError::SymbolNotFound(dep.name.clone()))?;
            
            // 验证 CRC（版本兼容性）
            if !SYMBOL_TABLE.get().unwrap().verify_crc(&dep.name, dep.crc) {
                return Err(ResolveError::VersionMismatch(dep.name));
            }
            
            resolved.insert(dep.name, addr);
        }
        
        Ok(resolved)
    }
}

#[repr(C)]
struct SymbolDependency {
    name: String,
    crc: u32,
    flags: u32,
}
```

---

### 5. 重定位引擎

```rust
// kernel/src/module/relocator.rs

pub struct Relocator;

impl Relocator {
    /// 应用重定位
    pub fn apply_relocations(
        &self,
        elf: &ElfParser,
        code_base: VirtAddr,
        symbols: &HashMap<String, usize>,
    ) -> Result<(), RelocError> {
        let reloc_section = elf.find_section(".axm_reloc")?;
        let relocations = self.parse_relocations(reloc_section)?;
        
        for reloc in relocations {
            let target_addr = code_base.as_u64() + reloc.offset;
            let symbol_addr = symbols.get(&reloc.symbol)
                .ok_or(RelocError::SymbolNotFound)?;
            
            match reloc.reloc_type {
                RelocType::Absolute64 => {
                    // 直接写入 64 位地址
                    unsafe {
                        *(target_addr as *mut u64) = *symbol_addr as u64;
                    }
                }
                
                RelocType::Relative32 => {
                    // PC 相对寻址（call 指令常用）
                    let relative = (*symbol_addr as i64) - (target_addr as i64);
                    if relative < i32::MIN as i64 || relative > i32::MAX as i64 {
                        return Err(RelocError::OutOfRange);
                    }
                    unsafe {
                        *(target_addr as *mut i32) = relative as i32;
                    }
                }
                
                RelocType::GotEntry => {
                    // GOT 表项（间接跳转）
                    self.setup_got_entry(target_addr, *symbol_addr)?;
                }
            }
        }
        
        Ok(())
    }
}

#[repr(C)]
struct Relocation {
    offset: u64,       // 需要修补的位置
    symbol: String,    // 符号名
    reloc_type: RelocType,
    addend: i64,       // 附加偏移
}

#[repr(u8)]
enum RelocType {
    Absolute64 = 0,
    Relative32 = 1,
    GotEntry = 2,
}
```

---

### 6. 模块编译工具链

**新增：axm-build 工具**

```rust
// tools/axm-build/src/main.rs

/// 编译 AXIS 模块
/// 
/// 用法：axm-build --input driver.rs --output driver.axm
fn main() {
    let args = parse_args();
    
    // 1. 编译 Rust 代码为目标文件
    let obj_file = compile_rust_to_obj(&args.input)?;
    
    // 2. 提取符号和重定位信息
    let symbols = extract_symbols(&obj_file)?;
    let relocs = extract_relocations(&obj_file)?;
    
    // 3. 生成 .axm 文件
    let mut axm = AxmBuilder::new();
    axm.add_code_section(obj_file.text())?;
    axm.add_data_section(obj_file.data())?;
    axm.add_relocation_section(&relocs)?;
    axm.add_dependency_section(&symbols)?;
    axm.add_metadata_section(&args.metadata)?;
    
    // 4. 写入输出文件
    axm.write_to_file(&args.output)?;
    
    println!("Module built: {}", args.output);
}

fn compile_rust_to_obj(input: &Path) -> Result<ObjectFile> {
    // 调用 rustc 编译
    let output = Command::new("rustc")
        .arg("--crate-type=staticlib")
        .arg("--target=x86_64-unknown-axis")  // 自定义 target
        .arg("-C").arg("relocation-model=pic") // 位置无关代码
        .arg("-C").arg("panic=abort")
        .arg("-o").arg("temp.o")
        .arg(input)
        .output()?;
    
    if !output.status.success() {
        return Err(BuildError::CompileFailed);
    }
    
    ObjectFile::load("temp.o")
}
```

**模块编写示例**：

```rust
// drivers/e1000/src/lib.rs

#![no_std]
#![feature(allocator_api)]

// 导入内核接口（通过符号解析）
extern "C" {
    fn alloc_page() -> Option<usize>;
    fn free_page(addr: usize);
    fn pci_register_driver(driver: *const PciDriver) -> i32;
}

// 模块元数据
#[used]
#[link_section = ".axm_meta"]
static MODULE_META: AxmMetadata = AxmMetadata {
    magic: 0x41584D00,
    version: 1,
    kernel_version: 1,
    name: *b"e1000\0...",
    author: *b"AXIS Team\0...",
    license: *b"GPL\0...",
    module_type: ModuleType::Driver,
    flags: 0,
};

// 驱动结构
static E1000_DRIVER: PciDriver = PciDriver {
    name: "e1000",
    id_table: &E1000_PCI_IDS,
    probe: e1000_probe,
    remove: e1000_remove,
};

// 初始化函数
#[no_mangle]
pub extern "C" fn module_init() -> i32 {
    unsafe {
        pci_register_driver(&E1000_DRIVER as *const _)
    }
}

// 清理函数
#[no_mangle]
pub extern "C" fn module_exit() {
    // 注销驱动
}

fn e1000_probe(dev: *mut PciDevice) -> i32 {
    // 探测设备
    0
}

fn e1000_remove(dev: *mut PciDevice) {
    // 移除设备
}
```

---

### 7. 系统调用接口

```rust
// kernel/src/syscall/module.rs

pub fn sys_load_module(path: *const u8, flags: u32) -> Result<ModuleId, SyscallError> {
    // 权限检查（需要 root）
    if !current_task().has_capability(CAP_SYS_MODULE) {
        return Err(SyscallError::PermissionDenied);
    }
    
    // 读取模块文件
    let path_str = unsafe { CStr::from_ptr(path as *const i8) }.to_str()?;
    let file_data = VFS.read_file(path_str)?;
    
    // 加载模块
    MODULE_LOADER.get().unwrap().load_module(&file_data)
}

pub fn sys_unload_module(module_id: ModuleId, flags: u32) -> Result<(), SyscallError> {
    if !current_task().has_capability(CAP_SYS_MODULE) {
        return Err(SyscallError::PermissionDenied);
    }
    
    MODULE_LOADER.get().unwrap().unload_module(module_id)
}

pub fn sys_list_modules(
    buffer: *mut ModuleInfo,
    count: usize,
) -> Result<usize, SyscallError> {
    // 列出已加载模块
    let modules = MODULE_LOADER.get().unwrap().list_modules();
    let to_copy = modules.len().min(count);
    
    unsafe {
        for (i, module) in modules.iter().take(to_copy).enumerate() {
            *buffer.add(i) = ModuleInfo {
                id: module.id,
                name: module.name.clone(),
                state: module.state,
                ref_count: module.ref_count.load(Ordering::Relaxed),
            };
        }
    }
    
    Ok(to_copy)
}
```

<br>



## 开发路线

### 阶段 1：基础设施（2-3 周）
- [ ] 实现符号导出系统
- [ ] 实现 ELF 解析器
- [ ] 实现基本的内存分配器（模块专用）
- [ ] 实现符号表和查找

### 阶段 2：加载器核心（3-4 周）
- [ ] 实现模块加载器框架
- [ ] 实现符号解析器
- [ ] 实现重定位引擎（x86-64）
- [ ] 实现版本检查机制

### 阶段 3：工具链（2 周）
- [ ] 开发 axm-build 编译工具
- [ ] 定义 .axm 文件格式规范
- [ ] 创建模块编写模板

### 阶段 4：驱动接口（2-3 周）
- [ ] 定义驱动框架接口
- [ ] 实现 PCI 驱动注册机制
- [ ] 移植一个简单驱动（e1000）作为示例

### 阶段 5：测试与优化（2 周）
- [ ] 单元测试
- [ ] 集成测试
- [ ] 性能优化
- [ ] 文档编写

<br>



## 挑战及解决方案

### 挑战 1：Rust 符号修饰
**问题**：Rust 编译器会修饰符号名（name mangling）
```rust
// 源代码
pub fn alloc_page() -> Option<PhysAddr>

// 编译后符号
_ZN4axis2mm3pmm10alloc_page17h8a9b...E
```

**解决方案**：
```rust
// 方案 A：使用 #[no_mangle] + extern "C"
#[no_mangle]
pub extern "C" fn alloc_page() -> Option<PhysAddr> { }

// 方案 B：建立符号映射表
export_symbol!(alloc_page => "_ZN4axis2mm3pmm10alloc_page...");
```

### 挑战 2：Rust 没有动态链接
**问题**：Rust 的静态编译模型不生成重定位信息

**解决方案**：
- 编译为 `staticlib` 并保留重定位信息
- 使用 `--emit=obj` 生成目标文件
- 通过 `llvm-objdump` 提取重定位表

### 挑战 3：内存安全
**问题**：加载外部代码破坏了 Rust 的安全保证

**解决方案**：
- 所有模块代码运行在 `unsafe` 上下文
- 实现严格的验证器（类似 eBPF verifier）

### 挑战 4：ABI 稳定性
**问题**：Rust 没有稳定的 ABI

**解决方案**：
- 内核只导出 `extern "C"` 函数
- 使用 C 风格的数据结构
- 版本号 + CRC 校验防止不兼容

<br>



## 参考资料

1. Linux 内核模块：`kernel/module.c`
2. ELF 规范：https://refspecs.linuxfoundation.org/elf/elf.pdf
3. Rust Unstable Book - `#[used]`
4. LLVM Relocation Types: https://llvm.org/docs/

<br>



## 总结

这个设计提供了：
- ✅ 运行时加载/卸载模块
- ✅ 符号导出和解析
- ✅ 版本兼容性检查
- ✅ 内存隔离
- ✅ 完整的工具链

**核心创新**：
1. 使用 `.axm` 格式而不是直接用 ELF
2. Rust 友好的符号导出宏
3. 编译时工具链（axm-build）

这个方案在保持 Rust 安全性的同时，实现了类似 Linux 的模块动态加载能力。
