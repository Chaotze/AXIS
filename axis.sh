#!/usr/bin/env bash
# ============================================================
# AXIS 构建脚本
# ============================================================
# 编译 bootloader 和内核，生成可引导的磁盘镜像
#
# 用法：
#   ./axis           - 构建可引导的 BIOS 镜像，并在 QEMU 中运行
#   ./axis build     - 构建可引导的 BIOS 镜像
#   ./axis run       - 在 QEMU 中运行可引导的 BIOS 镜像
#   ./axis clean     - 清理构建产物
#   ./axis rebuild   - 清理构建产物后构建并运行 BIOS 镜像
#   ./axis test      - 运行单元测试
#   ./axis help      - 显示帮助信息

set -e  # 遇到错误立即退出

# 颜色输出
RED='\033[0;31m'
GREEN='\033[0;32m'
YELLOW='\033[1;33m'
NC='\033[0m' # No Color

# 打印带颜色的消息
info() {
    echo -e "${GREEN}[INFO]${NC} $1"
}

warn() {
    echo -e "${YELLOW}[WARN]${NC} $1"
}

error() {
    echo -e "${RED}[ERROR]${NC} $1"
    exit 1
}

# 检查工具是否存在
test_tool() {
    if ! command -v $1 &> /dev/null; then
        error "$1 not found. Please install it first."
    fi
}

# 清理构建产物
clean() {
    info "Cleaning build artifacts..."
    cargo clean
    rm -rf target/
    info "Clean completed"
}

# 构建 BIOS 引导镜像
build() {
    info "Building BIOS boot image..."

    ############################################################
    # Stage1
    ############################################################
    info "Building BIOS Stage1 (MBR)..."
    test_tool nasm
    nasm -f bin bootloader/bios/stage1.asm -o target/x86_64-unknown-bios/stage1
    info "Stage1 built: target/x86_64-unknown-bios/stage1 (512 bytes)"

    ############################################################
    # Stage2
    ############################################################
    info "Building BIOS Stage2..."

    # 编译汇编部分
    nasm -f elf64 bootloader/bios/stage2.asm -o target/x86_64-unknown-bios/stage2_asm

    # 编译 Rust 部分
    pushd bootloader/bios && cargo +nightly build --release && popd

    # 链接 Bootloader 的 Stage2
    test_tool rust-lld
    rust-lld -flavor gnu -T bootloader/bios/stage2.ld \
        target/x86_64-unknown-bios/stage2_asm \
        target/x86_64-unknown-axis/release/libaxis_bootloader_bios.a \
        -o target/x86_64-unknown-bios/stage2

    info "Stage2 built: target/x86_64-unknown-bios/stage2"

    ############################################################
    # Kernel
    ############################################################
    info "Building kernel..."
    pushd kernel && cargo +nightly build --release && popd
    info "Kernel built: target/x86_64-unknown-axis/release/axis-kernel"

    ############################################################
    # Disk Image
    ############################################################
    info "Creating disk image..."

    # 创建磁盘镜像（0.5MB = 1024 扇区）
    # 容量依据：stage1 分 6 轮装载内核（128 扇区起，每轮 128 扇区），
    # 需 128 + 6×128 = 896 扇区 < 1024；0.25MB（512 扇区）已不足
    IMG_PATH="target/axis-0.1.2-bios-x86_64.img"
    dd if=/dev/zero of="$IMG_PATH" bs=1024 count=512 2>/dev/null
    info "Image file created: $IMG_PATH"

    # 写入 Stage1（MBR）
    info "Burning Stage1..."
    STAGE1_PATH="target/x86_64-unknown-bios/stage1"
    dd if="$STAGE1_PATH" of="$IMG_PATH" conv=notrunc bs=512 count=1 2>/dev/null
    info "Stage1 burned ($(stat -c%s "$STAGE1_PATH" 2>/dev/null || stat -f%z "$STAGE1_PATH" 2>/dev/null) bytes)"

    # 写入 Stage2（从扇区 2 开始，偏移 512 字节）
    info "Burning Stage2..."
    STAGE2_PATH="target/x86_64-unknown-bios/stage2"
    dd if="$STAGE2_PATH" of="$IMG_PATH" conv=notrunc bs=512 seek=1 2>/dev/null
    info "Stage2 burned ($(stat -c%s "$STAGE2_PATH" 2>/dev/null || stat -f%z "$STAGE2_PATH" 2>/dev/null) bytes, offset: 512)"

    # 写入 Kernel（从扇区 128 开始，偏移 64KB）
    info "Burning Kernel..."
    KERNEL_PATH="target/x86_64-unknown-axis/release/axis-kernel"
    dd if="$KERNEL_PATH" of="$IMG_PATH" conv=notrunc bs=512 seek=128 2>/dev/null
    info "Kernel burned ($(stat -c%s "$KERNEL_PATH" 2>/dev/null || stat -f%z "$KERNEL_PATH" 2>/dev/null) bytes, offset: $((128 * 512)))"

    info "BIOS image created: $IMG_PATH"
}

# 启动 qemu-system-x86_64 模拟器
run() {
    test_tool qemu-system-x86_64

    qemu_args=(
        qemu-system-x86_64
        -cpu max
        -drive format=raw,file=target/axis-0.1.2-bios-x86_64.img
        -display curses
        -m 128M -no-reboot -no-shutdown
    )
    qemu_cmd="${qemu_args[*]}"

    kitty_args=(
        @ launch
        --type=tab
        --cwd=current
        --title="axis-0.1.2-bios-x86_64"
        -- bash -c "$qemu_cmd; exec bash"
    )
    kitty "${kitty_args[@]}"
}

# 单元测试
test() {
    cargo test --package unitest --lib
}

# 帮助信息
help() {
    info "AXIS Build System"
	info ""
	info "Targets:"
	info "  ./axis          - Build bootable BIOS image and launch in QEMU"
	info "  ./axis build    - Build bootable BIOS image"
	info "  ./axis run      - Launch the previously built BIOS image in QEMU"
	info "  ./axis clean    - Remove all build artifacts"
	info "  ./axis rebuild  - Fully rebuild: clean + build + run"
	info "  ./axis test     - Run unit tests"
	info "  ./axis help     - Show this help message"
}

# 主函数
main() {
    # 创建 target 目录
    mkdir -p target/x86_64-unknown-bios

    case "${1:-auto}" in
        build)
            build
            ;;
        run)
            run
            ;;
        clean)
            clean
            ;;
        rebuild)
            clean
            mkdir -p target/x86_64-unknown-bios
            build
            run
            ;;
        test)
            test
            ;;
        help)
            help
            ;;
        auto|*)
            build
            run
            ;;
    esac
}

main "$@"
