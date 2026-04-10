#!/bin/bash
# iOS/tvOS编译脚本
# 用于编译vnt-Redir的iOS/tvOS静态库

set -e

# 颜色输出
RED='\033[0;31m'
GREEN='\033[0;32m'
YELLOW='\033[1;33m'
NC='\033[0m' # No Color

echo -e "${GREEN}VNT iOS/tvOS 编译脚本${NC}"
echo "================================"

# 检查是否安装了Rust
if ! command -v cargo &> /dev/null; then
    echo -e "${RED}错误: 未找到cargo，请先安装Rust${NC}"
    exit 1
fi

# 检查是否安装了iOS工具链
check_target() {
    local target=$1
    if ! rustup target list | grep -q "$target (installed)"; then
        echo -e "${YELLOW}安装目标: $target${NC}"
        rustup target add "$target"
    fi
}

# 编译函数
build_target() {
    local target=$1
    local name=$2
    
    echo -e "${GREEN}编译 $name ($target)...${NC}"
    cargo build --release --target "$target" --features integrated_tun
    
    if [ $? -eq 0 ]; then
        echo -e "${GREEN}✓ $name 编译成功${NC}"
    else
        echo -e "${RED}✗ $name 编译失败${NC}"
        exit 1
    fi
}

# 创建输出目录
OUTPUT_DIR="./ios-build"
mkdir -p "$OUTPUT_DIR"

# iOS目标
IOS_TARGETS=(
    "aarch64-apple-ios:iOS (ARM64)"
    "x86_64-apple-ios:iOS Simulator (x86_64)"
    "aarch64-apple-ios-sim:iOS Simulator (ARM64)"
)

# tvOS目标
TVOS_TARGETS=(
    "aarch64-apple-tvos:tvOS (ARM64)"
    "x86_64-apple-tvos:tvOS Simulator (x86_64)"
    "aarch64-apple-tvos-sim:tvOS Simulator (ARM64)"
)

# 选择编译目标
echo ""
echo "请选择编译目标:"
echo "1) iOS (所有架构)"
echo "2) iOS (仅设备 ARM64)"
echo "3) iOS (仅模拟器)"
echo "4) tvOS (所有架构)"
echo "5) tvOS (仅设备 ARM64)"
echo "6) 全部"
echo ""
read -p "请输入选项 (1-6): " choice

case $choice in
    1)
        TARGETS=("${IOS_TARGETS[@]}")
        ;;
    2)
        TARGETS=("aarch64-apple-ios:iOS (ARM64)")
        ;;
    3)
        TARGETS=(
            "x86_64-apple-ios:iOS Simulator (x86_64)"
            "aarch64-apple-ios-sim:iOS Simulator (ARM64)"
        )
        ;;
    4)
        TARGETS=("${TVOS_TARGETS[@]}")
        ;;
    5)
        TARGETS=("aarch64-apple-tvos:tvOS (ARM64)")
        ;;
    6)
        TARGETS=("${IOS_TARGETS[@]}" "${TVOS_TARGETS[@]}")
        ;;
    *)
        echo -e "${RED}无效选项${NC}"
        exit 1
        ;;
esac

# 编译所有选定的目标
for target_info in "${TARGETS[@]}"; do
    IFS=':' read -r target name <<< "$target_info"
    check_target "$target"
    build_target "$target" "$name"
done

# 复制静态库到输出目录
echo ""
echo -e "${GREEN}复制静态库到输出目录...${NC}"

for target_info in "${TARGETS[@]}"; do
    IFS=':' read -r target name <<< "$target_info"
    
    # 确定平台名称
    if [[ $target == *"ios"* ]]; then
        platform="ios"
    else
        platform="tvos"
    fi
    
    # 确定架构
    if [[ $target == "aarch64"* ]]; then
        arch="arm64"
    else
        arch="x86_64"
    fi
    
    # 确定是否为模拟器
    if [[ $target == *"sim"* ]]; then
        variant="simulator"
    else
        variant="device"
    fi
    
    # 创建目标目录
    dest_dir="$OUTPUT_DIR/$platform/$variant/$arch"
    mkdir -p "$dest_dir"
    
    # 复制静态库
    src_lib="./target/$target/release/libvnt.a"
    if [ -f "$src_lib" ]; then
        cp "$src_lib" "$dest_dir/"
        echo -e "${GREEN}✓ 已复制: $dest_dir/libvnt.a${NC}"
    else
        echo -e "${YELLOW}⚠ 未找到: $src_lib${NC}"
    fi
done

# 创建通用二进制（如果编译了多个架构）
echo ""
echo -e "${GREEN}创建通用二进制...${NC}"

create_universal_binary() {
    local platform=$1
    local variant=$2
    local output_name=$3
    
    local libs=()
    for arch in arm64 x86_64; do
        local lib="$OUTPUT_DIR/$platform/$variant/$arch/libvnt.a"
        if [ -f "$lib" ]; then
            libs+=("$lib")
        fi
    done
    
    if [ ${#libs[@]} -gt 1 ]; then
        local output="$OUTPUT_DIR/$platform/$variant/$output_name"
        lipo -create "${libs[@]}" -output "$output"
        echo -e "${GREEN}✓ 已创建通用二进制: $output${NC}"
    fi
}

# 为iOS创建通用二进制
if ls "$OUTPUT_DIR/ios/device/"* 1> /dev/null 2>&1; then
    create_universal_binary "ios" "device" "libvnt-universal.a"
fi

if ls "$OUTPUT_DIR/ios/simulator/"* 1> /dev/null 2>&1; then
    create_universal_binary "ios" "simulator" "libvnt-universal.a"
fi

# 为tvOS创建通用二进制
if ls "$OUTPUT_DIR/tvos/device/"* 1> /dev/null 2>&1; then
    create_universal_binary "tvos" "device" "libvnt-universal.a"
fi

if ls "$OUTPUT_DIR/tvos/simulator/"* 1> /dev/null 2>&1; then
    create_universal_binary "tvos" "simulator" "libvnt-universal.a"
fi

# 复制头文件
echo ""
echo -e "${GREEN}复制头文件...${NC}"
cp "./documents/VNT-Bridging-Header.h" "$OUTPUT_DIR/"
echo -e "${GREEN}✓ 已复制: $OUTPUT_DIR/VNT-Bridging-Header.h${NC}"

# 生成使用说明
cat > "$OUTPUT_DIR/README.txt" << EOF
VNT iOS/tvOS 静态库
==================

编译时间: $(date)

目录结构:
---------
ios/
  device/
    arm64/libvnt.a          - iOS设备 (ARM64)
    libvnt-universal.a      - iOS设备通用二进制
  simulator/
    arm64/libvnt.a          - iOS模拟器 (ARM64)
    x86_64/libvnt.a         - iOS模拟器 (x86_64)
    libvnt-universal.a      - iOS模拟器通用二进制

tvos/
  device/
    arm64/libvnt.a          - tvOS设备 (ARM64)
  simulator/
    arm64/libvnt.a          - tvOS模拟器 (ARM64)
    x86_64/libvnt.a         - tvOS模拟器 (x86_64)

VNT-Bridging-Header.h       - Swift桥接头文件

使用方法:
---------
1. 将对应平台的libvnt.a添加到Xcode项目
2. 将VNT-Bridging-Header.h设置为桥接头文件
3. 参考documents/iOS-Integration.md进行集成

注意事项:
---------
- 设备和模拟器使用不同的静态库
- 如果需要同时支持多个架构，使用通用二进制
- 确保在Xcode中正确配置链接器标志

更多信息请参考: documents/iOS-Integration.md
EOF

echo ""
echo -e "${GREEN}================================${NC}"
echo -e "${GREEN}编译完成！${NC}"
echo -e "${GREEN}输出目录: $OUTPUT_DIR${NC}"
echo ""
echo "下一步:"
echo "1. 查看 $OUTPUT_DIR/README.txt"
echo "2. 参考 documents/iOS-Integration.md 进行集成"
echo ""
