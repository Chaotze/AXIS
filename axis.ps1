#
# ============================================================
# AXIS 构建脚本（Windows PowerShell）
# ============================================================
# 编译 bootloader 和内核，生成可引导的磁盘镜像
#
# 用法：
#   .\axis           - 构建可引导的 BIOS 镜像，并在 QEMU 中运行
#   .\axis build     - 构建可引导的 BIOS 镜像
#   .\axis run       - 在 QEMU 中运行可引导的 BIOS 镜像
#   .\axis clean     - 清理构建产物
#   .\axis rebuild   - 清理构建产物后构建并运行 BIOS 镜像
#   .\axis test      - 运行单元测试
#   .\axis help      - 显示帮助信息

param(
    [string]$Target
)

$ErrorActionPreference = "Stop"

# 颜色输出函数
function Write-Info {
    param([string]$Message)
    Write-Host "[INFO] " -ForegroundColor Green -NoNewline
    Write-Host $Message
}

function Write-Warn {
    param([string]$Message)
    Write-Host "[WARN] " -ForegroundColor Yellow -NoNewline
    Write-Host $Message
}

function Write-Error {
    param([string]$Message)
    Write-Host "[ERROR] " -ForegroundColor Red -NoNewline
    Write-Host $Message
    exit 1
}

# 检查工具是否存在
function Test-Tool {
    param([string]$Name)
    if (-not (Get-Command $Name -ErrorAction SilentlyContinue)) {
        Write-Error "$Name not found. Please install it first."
    }
}

# 清理构建产物
function Invoke-Clean {
    Write-Info "Cleaning build artifacts..."
    cargo clean
    Remove-Item -Recurse -Force -ErrorAction SilentlyContinue target
    Write-Info "Clean completed"
}

# 构建 BIOS 引导镜像
function Invoke-Build {
    Write-Info "Building BIOS boot image..."
    $axisPath = Get-Location

    ############################################################
    # Stage1
    ############################################################
    Write-Info "Building BIOS Stage1 (MBR)..."
    Test-Tool nasm
    nasm -f bin bootloader\bios\stage1.asm -o target\x86_64-unknown-bios\stage1
    Write-Info "Stage1 built: target\x86_64-unknown-bios\stage1 (512 bytes)"

    ############################################################
    # Stage2
    ############################################################
    Write-Info "Building BIOS Stage2..."

    # 编译汇编部分
    nasm -f elf64 bootloader\bios\stage2.asm -o target\x86_64-unknown-bios\stage2_asm

    # 编译 Rust 部分
    Push-Location bootloader\bios; cargo +nightly build --release; Pop-Location

    # 链接 Bootloader 的 Stage2
    Test-Tool rust-lld
    rust-lld -flavor gnu -T bootloader\bios\stage2.ld `
        target\x86_64-unknown-bios\stage2_asm `
        target\x86_64-unknown-axis\release\libaxis_bootloader_bios.a `
        -o target\x86_64-unknown-bios\stage2

    Write-Info "Stage2 built: target\x86_64-unknown-bios\stage2"

    ############################################################
    # Kernel
    ############################################################
    Write-Info "Building kernel..."
    Push-Location kernel; cargo +nightly build --release; Pop-Location
    Write-Info "Kernel built: target\x86_64-unknown-axis\release\axis-kernel"

    ############################################################
    # Disk Image
    ############################################################
    Write-Info "Creating disk image..."

    # 创建磁盘镜像（0.5MB，1024 扇区）
    $imgSize = 0.5 * 1024 * 1024
    $bytes = New-Object byte[] $imgSize
    $imgPath = Join-Path $axisPath "target\axis-0.2.0-bios-x86_64.img"
    [System.IO.File]::WriteAllBytes($imgPath, $bytes)
    Write-Info "Image file created: $imgPath"

    # 写入 Stage1（MBR）
    Write-Info "Burning Stage1..."
    $stage1Path = Join-Path $axisPath "target\x86_64-unknown-bios\stage1"
    $stage1 = [System.IO.File]::ReadAllBytes($stage1Path)
    $img = [System.IO.File]::Open($imgPath, [System.IO.FileMode]::Open)
    $img.Write($stage1, 0, $stage1.Length)
    Write-Info "Stage1 burned ($($stage1.Length) bytes)"

    # 写入 Stage2（从扇区 2 开始，偏移 512 字节）
    Write-Info "Burning Stage2..."
    $stage2Path = Join-Path $axisPath "target\x86_64-unknown-bios\stage2"
    $stage2 = [System.IO.File]::ReadAllBytes($stage2Path)
    $img.Seek(512, [System.IO.SeekOrigin]::Begin) | Out-Null
    $img.Write($stage2, 0, $stage2.Length)
    Write-Info "Stage2 burned ($($stage2.Length) bytes, offset: 512)"

    # 写入 Kernel（从扇区 128 开始，偏移 64KB）
    Write-Info "Burning Kernel..."
    $kernelPath = Join-Path $axisPath "target\x86_64-unknown-axis\release\axis-kernel"
    $kernel = [System.IO.File]::ReadAllBytes($kernelPath)
    $img.Seek(128 * 512, [System.IO.SeekOrigin]::Begin) | Out-Null
    $img.Write($kernel, 0, $kernel.Length)
    Write-Info "Kernel burned ($($kernel.Length) bytes, offset: $((128 * 512)))"

    $img.Close()
    Write-Info "BIOS image created: $imgPath"
}

# 启动 qemu-system-x86_64 模拟器
function Invoke-Run {
    Test-Tool qemu-system-x86_64

    $qemuCmdParts = @(
        'qemu-system-x86_64',
        '-cpu max',
        '-drive format=raw,file=target\axis-0.2.0-bios-x86_64.img',
        '-display curses',
        '-m 128M -no-reboot -no-shutdown'
    )
    $qemuCmd = $qemuCmdParts -join ' '
    wt -w 0 new-tab `
        -d . `
        -p "Windows PowerShell" `
        --title "axis-0.2.0-bios-x86_64" `
        -- powershell -NoExit -Command "& { $qemuCmd }"
}

# 单元测试
function Invoke-Test {
    cargo test --package unitest --lib
}

# 帮助信息
function Invoke-Help {
    Write-Info "AXIS Build System"
	Write-Info ""
	Write-Info "Targets:"
	Write-Info "  .\axis          - Build bootable BIOS image and launch in QEMU"
	Write-Info "  .\axis build    - Build bootable BIOS image"
	Write-Info "  .\axis run      - Launch the previously built BIOS image in QEMU"
	Write-Info "  .\axis clean    - Remove all build artifacts"
	Write-Info "  .\axis rebuild  - Fully rebuild: clean + build + run"
	Write-Info "  .\axis test     - Run unit tests"
	Write-Info "  .\axis help     - Show this help message"
}

# 主函数
function Main {
    # 创建 target 目录
    New-Item -ItemType Directory -Force -Path "target\x86_64-unknown-bios" | Out-Null

    switch ($Target) {
        "build" {
            Invoke-Build
        }
        "run" {
            Invoke-Run
        }
        "clean" {
            Invoke-Clean
        }
        "rebuild" {
            Invoke-Clean
            New-Item -ItemType Directory -Force -Path "target\x86_64-unknown-bios" | Out-Null
            Invoke-Build
            Invoke-Run
        }
        "test" {
            Invoke-Test
        }
        "help" {
            Invoke-Help
        }
        default {
            Invoke-Build
            Invoke-Run
        }
    }
}

# 执行主函数
Main
