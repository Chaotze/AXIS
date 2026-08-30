// ============================================================
// 简单的内核 Shell
// ============================================================
// 为什么需要 Shell：
// - 验证 VFS 功能是否正常工作
// - 提供交互式的文件系统访问
// - 支持 cat、ls 等基本命令

use alloc::string::{String, ToString};
use alloc::vec::Vec;
use crate::fs::vfs::FileSystem;  // 导入 FileSystem trait

// ============================================================
// 输入读取
// ============================================================

/// 从演示命令列表读取输入
fn read_line() -> String {
    static DEMO_COMMANDS: &[&str] = &[
        "help",
        "ls /",
        "ls /dev",
        "cat /proc/cpuinfo",
        "cat /proc/meminfo",
        "exit",
    ];

    static mut CURRENT_CMD: usize = 0;

    let cmd_line = unsafe {
        let cmd = if CURRENT_CMD < DEMO_COMMANDS.len() {
            let c = DEMO_COMMANDS[CURRENT_CMD];
            CURRENT_CMD += 1;
            c
        } else {
            CURRENT_CMD = 0;
            DEMO_COMMANDS[0]
        };
        cmd
    };

    println!("{}", cmd_line);
    cmd_line.to_string()
}

// ============================================================
// 命令解析
// ============================================================

/// 解析命令行
fn parse_command(line: &str) -> Option<(String, Vec<String>)> {
    let trimmed = line.trim();
    if trimmed.is_empty() {
        return None;
    }

    let mut parts = trimmed.split_whitespace();
    let cmd = parts.next()?.to_string();
    let args = parts.map(|s| s.to_string()).collect();

    Some((cmd, args))
}

// ============================================================
// 命令处理
// ============================================================

/// 执行 cat 命令
fn cmd_cat(args: &[String]) -> Result<(), &'static str> {
    if args.is_empty() {
        return Err("cat: missing file argument");
    }

    let filepath = &args[0];

    // 为了演示，我们直接访问各个文件系统
    // 实际系统应该通过 VFS 挂载点穿越实现

    // 获取所有文件系统实例
    let guard = super::VFS_STATE.lock();
    let state = guard.as_ref().ok_or("VFS not initialized")?;

    // 根据路径前缀选择合适的文件系统
    if filepath.starts_with("/proc/") {
        // 从 procfs 读取
        let filename = &filepath[6..];  // 去掉 "/proc/"

        // 创建临时 procfs 实例来查找文件
        drop(guard);  // 释放锁

        match super::filesystems::Procfs::new() {
            Ok(procfs) => {
                // 查找文件
                let file_ino = procfs.lookup(procfs.root_inode(), filename.as_bytes())
                    .map_err(|_| "file not found")?;

                // 读取文件
                let mut buf = [0u8; 4096];
                let n = procfs.read(file_ino, 0, &mut buf)
                    .map_err(|_| "failed to read file")?;

                if let Ok(content) = core::str::from_utf8(&buf[..n]) {
                    print!("{}", content);
                } else {
                    println!("[Binary data: {} bytes]", n);
                }
                Ok(())
            }
            Err(_) => Err("procfs not available"),
        }
    } else if filepath.starts_with("/dev/") {
        // 从 devfs 读取
        let filename = &filepath[5..];  // 去掉 "/dev/"

        drop(guard);

        match super::filesystems::Devfs::new() {
            Ok(devfs) => {
                let file_ino = devfs.lookup(devfs.root_inode(), filename.as_bytes())
                    .map_err(|_| "file not found")?;

                let mut buf = [0u8; 4096];
                let n = devfs.read(file_ino, 0, &mut buf)
                    .map_err(|_| "failed to read file")?;

                if let Ok(content) = core::str::from_utf8(&buf[..n]) {
                    print!("{}", content);
                } else {
                    println!("[Binary data: {} bytes]", n);
                }
                Ok(())
            }
            Err(_) => Err("devfs not available"),
        }
    } else if filepath.starts_with("/sys/") {
        // 从 sysfs 读取
        let filename = &filepath[5..];  // 去掉 "/sys/"

        drop(guard);

        match super::filesystems::Sysfs::new() {
            Ok(sysfs) => {
                let file_ino = sysfs.lookup(sysfs.root_inode(), filename.as_bytes())
                    .map_err(|_| "file not found")?;

                let mut buf = [0u8; 4096];
                let n = sysfs.read(file_ino, 0, &mut buf)
                    .map_err(|_| "failed to read file")?;

                if let Ok(content) = core::str::from_utf8(&buf[..n]) {
                    print!("{}", content);
                } else {
                    println!("[Binary data: {} bytes]", n);
                }
                Ok(())
            }
            Err(_) => Err("sysfs not available"),
        }
    } else {
        // 从 tmpfs (root_fs) 读取
        let root_fs = state.root_fs.as_ref().ok_or("No root filesystem mounted")?.clone();
        drop(guard);

        let path_bytes = filepath.as_bytes();
        if !path_bytes.starts_with(b"/") {
            return Err("path must be absolute");
        }

        let path = &path_bytes[1..];
        let parts: Vec<&[u8]> = path.split(|&b| b == b'/').filter(|p| !p.is_empty()).collect();

        if parts.is_empty() {
            return Err("invalid path");
        }

        let mut current_ino = root_fs.root_inode();

        for (idx, part) in parts.iter().enumerate() {
            if idx == parts.len() - 1 {
                current_ino = root_fs.lookup(current_ino, part)
                    .map_err(|_| "file not found")?;
                break;
            } else {
                current_ino = root_fs.lookup(current_ino, part)
                    .map_err(|_| "directory not found")?;
            }
        }

        let mut buf = [0u8; 4096];
        let n = root_fs.read(current_ino, 0, &mut buf)
            .map_err(|_| "failed to read file")?;

        if let Ok(content) = core::str::from_utf8(&buf[..n]) {
            print!("{}", content);
        } else {
            println!("[Binary data: {} bytes]", n);
        }

        Ok(())
    }
}

/// 执行 ls 命令
fn cmd_ls(args: &[String]) -> Result<(), &'static str> {
    let path = if args.is_empty() {
        "/"
    } else {
        &args[0]
    };

    // 根据路径前缀选择合适的文件系统
    let guard = super::VFS_STATE.lock();
    let state = guard.as_ref().ok_or("VFS not initialized")?;

    if path.starts_with("/proc") {
        drop(guard);
        match super::filesystems::Procfs::new() {
            Ok(procfs) => {
                let entries = procfs.readdir(procfs.root_inode())
                    .map_err(|_| "failed to read directory")?;

                for entry in entries {
                    let name_str = entry.name_str().unwrap_or("?");
                    let type_str = match entry.file_type {
                        crate::fs::FileType::Directory => "(dir)",
                        crate::fs::FileType::File => "(file)",
                        _ => "(?)",
                    };
                    println!("  {:<20} {}", name_str, type_str);
                }
                Ok(())
            }
            Err(_) => Err("procfs not available"),
        }
    } else if path.starts_with("/dev") {
        drop(guard);
        match super::filesystems::Devfs::new() {
            Ok(devfs) => {
                let entries = devfs.readdir(devfs.root_inode())
                    .map_err(|_| "failed to read directory")?;

                for entry in entries {
                    let name_str = entry.name_str().unwrap_or("?");
                    let type_str = match entry.file_type {
                        crate::fs::FileType::CharDevice => "(char)",
                        crate::fs::FileType::BlockDevice => "(block)",
                        crate::fs::FileType::Directory => "(dir)",
                        _ => "(?)",
                    };
                    println!("  {:<20} {}", name_str, type_str);
                }
                Ok(())
            }
            Err(_) => Err("devfs not available"),
        }
    } else if path.starts_with("/sys") {
        drop(guard);
        match super::filesystems::Sysfs::new() {
            Ok(sysfs) => {
                let entries = sysfs.readdir(sysfs.root_inode())
                    .map_err(|_| "failed to read directory")?;

                for entry in entries {
                    let name_str = entry.name_str().unwrap_or("?");
                    let type_str = match entry.file_type {
                        crate::fs::FileType::Directory => "(dir)",
                        _ => "(?)",
                    };
                    println!("  {:<20} {}", name_str, type_str);
                }
                Ok(())
            }
            Err(_) => Err("sysfs not available"),
        }
    } else {
        // 从 tmpfs (root_fs) 读取
        let root_fs = state.root_fs.as_ref().ok_or("No root filesystem mounted")?.clone();
        drop(guard);

        let path_bytes = path.as_bytes();
        let mut current_ino = root_fs.root_inode();

        if path_bytes != b"/" {
            let path = if path_bytes.starts_with(b"/") {
                &path_bytes[1..]
            } else {
                path_bytes
            };

            for part in path.split(|&b| b == b'/').filter(|p| !p.is_empty()) {
                current_ino = root_fs.lookup(current_ino, part)
                    .map_err(|_| "directory not found")?;
            }
        }

        let entries = root_fs.readdir(current_ino)
            .map_err(|_| "failed to read directory")?;

        for entry in entries {
            let name_str = entry.name_str().unwrap_or("?");
            let type_str = match entry.file_type {
                crate::fs::FileType::Directory => "(dir)",
                crate::fs::FileType::File => "(file)",
                crate::fs::FileType::CharDevice => "(char)",
                crate::fs::FileType::BlockDevice => "(block)",
                _ => "(?)",
            };
            println!("  {:<20} {}", name_str, type_str);
        }

        Ok(())
    }
}

/// 执行 help 命令
fn cmd_help() {
    println!("Available commands:");
    println!("  cat <file>    - Display file contents");
    println!("  ls [path]     - List directory contents");
    println!("  pwd           - Print working directory (/)");
    println!("  clear         - Clear the screen");
    println!("  help          - Show this help message");
    println!("  exit          - Exit the shell");
}

/// 执行 pwd 命令
fn cmd_pwd() {
    println!("/");
}

// ============================================================
// 主 Shell 循环
// ============================================================

/// 启动交互式 Shell
pub fn shell_loop() -> ! {
    loop {
        // 显示提示符
        print!("root@axis:~$ ");

        // 读取用户输入
        let cmd_line = read_line();

        // 解析并执行命令
        if let Some((cmd, args)) = parse_command(&cmd_line) {
            let result = match cmd.as_str() {
                "cat" => cmd_cat(&args),
                "ls" => cmd_ls(&args),
                "pwd" => { cmd_pwd(); Ok(()) },
                "help" => { cmd_help(); Ok(()) },
                "clear" => {
                    crate::lib::vga::clear_screen();
                    Ok(())
                }
                "exit" => {
                    println!("\nroot@axis:~$ ");
                    // 在实际内核中会进行清理工作
                    loop {
                        unsafe { core::arch::asm!("hlt"); }
                    }
                }
                "" => Ok(()),
                _ => Err("unknown command"),
            };

            if let Err(e) = result {
                println!("[Error] {}", e);
            }
            
            for _ in 0..(2 + 1) * 5_000_000 {
                // if i % 5_000_000 == 0 && i > 0 {
                //     print!(".");
                // }
                unsafe {
                    core::arch::asm!("pause");
                }
            }
        }

        println!();
    }
}
