#
# ============================================================
# AXIS Makefile
# ============================================================
# 构建系统主入口
#
# 用法：
#   make             - 构建可引导的 BIOS 镜像，并在 QEMU 中运行
#   make build       - 同上
#   make run         - 在 QEMU 中运行可引导的 BIOS 镜像
#   make clean       - 清理构建产物
#   make rebuild     - 清理构建产物后构建并运行 BIOS 镜像
#   make help        - 显示帮助信息

.PHONY: all build run clean rebuild help

# 默认目标：构建可引导的 BIOS 镜像，并在 QEMU 中运行
all:
	@bash axis.sh

# 同上
build:
	@bash axis.sh build

# 在 QEMU 中运行可引导的 BIOS 镜像
run:
	@bash axis.sh run

# 清理构建产物
clean:
	@bash axis.sh clean

# 清理构建产物后构建并运行 BIOS 镜像
rebuild:
	@bash axis.sh rebuild

# 帮助信息
help:
	@echo "AXIS Build System"
	@echo ""
	@echo "Targets:"
	@echo "  make            - Build bootable BIOS image and launch in QEMU"
	@echo "  make build      - Alias for the default target"
	@echo "  make run        - Launch the previously built BIOS image in QEMU"
	@echo "  make clean      - Remove all build artifacts"
	@echo "  make rebuild    - Fully rebuild: clean + build + run"
	@echo "  make help       - Show this help message"
