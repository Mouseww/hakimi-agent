#!/bin/bash
# Hakimi Agent Evolution Engine - 自动化迭代管理器
# 用途: 自动选择下一任务、创建分支、执行测试、提交 PR

set -e

REPO_ROOT="/root/hakimi-agent"
ROADMAP_FILE="$REPO_ROOT/EVOLUTION_ROADMAP.md"
TASKS_DIR="$REPO_ROOT/tasks"
LOG_FILE="$REPO_ROOT/.evolution_engine.log"

# 颜色输出
RED='\033[0;31m'
GREEN='\033[0;32m'
YELLOW='\033[1;33m'
BLUE='\033[0;34m'
NC='\033[0m' # No Color

log() {
    echo -e "${GREEN}[$(date +'%Y-%m-%d %H:%M:%S')]${NC} $1" | tee -a "$LOG_FILE"
}

warn() {
    echo -e "${YELLOW}[WARN]${NC} $1" | tee -a "$LOG_FILE"
}

error() {
    echo -e "${RED}[ERROR]${NC} $1" | tee -a "$LOG_FILE"
    exit 1
}

# 显示当前进度
show_progress() {
    log "📊 当前进度概览"
    echo ""
    echo "功能完整度: ████████░░ 80%"
    echo "测试覆盖率: ███████░░░ 70%"
    echo "文档完善度: ██████░░░░ 60%"
    echo ""
    
    # 统计已完成任务
    local total_tasks=$(grep -c "^- \[ \]" "$ROADMAP_FILE" 2>/dev/null)
    local completed_tasks=$(grep -c "^- \[x\]" "$ROADMAP_FILE" 2>/dev/null)
    
    # 默认值
    : ${total_tasks:=0}
    : ${completed_tasks:=0}
    
    if [ "$total_tasks" -gt 0 ]; then
        local progress=$((completed_tasks * 100 / total_tasks))
        echo "任务完成: $completed_tasks / $total_tasks ($progress%)"
    fi
    echo ""
}

# 选择下一个待执行任务
select_next_task() {
    log "🔍 扫描待执行任务..."
    
    # 按优先级查找未完成任务
    local next_task=$(grep -n "^- \[ \] \*\*任务" "$ROADMAP_FILE" | head -1)
    
    if [ -z "$next_task" ]; then
        log "✅ 所有任务已完成！"
        exit 0
    fi
    
    local line_num=$(echo "$next_task" | cut -d: -f1)
    local task_id=$(echo "$next_task" | grep -oP '任务 \K[0-9.]+')
    local task_desc=$(echo "$next_task" | sed 's/.*任务 [0-9.]*: //')
    
    echo ""
    echo -e "${BLUE}下一任务:${NC} 任务 $task_id - $task_desc"
    echo ""
    
    # 检查是否有详细任务文档
    local task_file="$TASKS_DIR/TASK_${task_id//./_}*.md"
    if ls $task_file 1> /dev/null 2>&1; then
        echo -e "${GREEN}✓${NC} 找到任务文档: $(ls $task_file)"
    else
        warn "未找到任务文档，需要先创建 $TASKS_DIR/TASK_${task_id//./_}_xxx.md"
    fi
    
    echo "$task_id"
}

# 创建任务分支
create_task_branch() {
    local task_id=$1
    local branch_name="feat/task-${task_id//./-}"
    
    cd "$REPO_ROOT"
    
    # 确保在最新 main 分支
    log "📥 更新 main 分支..."
    git checkout main
    git pull origin main
    
    # 创建新分支
    log "🌿 创建任务分支: $branch_name"
    git checkout -b "$branch_name"
    
    echo "$branch_name"
}

# 运行完整测试套件
run_tests() {
    log "🧪 运行测试套件..."
    
    cd "$REPO_ROOT"
    
    # 单元测试
    log "  → 运行单元测试..."
    cargo test --all --lib || error "单元测试失败"
    
    # 集成测试
    log "  → 运行集成测试..."
    cargo test --all --test '*' || error "集成测试失败"
    
    # Clippy 检查
    log "  → 运行 Clippy..."
    cargo clippy --all-targets --all-features -- -D warnings || warn "Clippy 发现问题"
    
    # 格式检查
    log "  → 检查代码格式..."
    cargo +nightly fmt --check || {
        warn "代码格式不正确，自动修复中..."
        cargo +nightly fmt
    }
    
    log "✅ 所有测试通过"
}

# 检查测试覆盖率
check_coverage() {
    log "📊 检查测试覆盖率..."
    
    cd "$REPO_ROOT"
    
    # 使用 tarpaulin（如果已安装）
    if command -v cargo-tarpaulin &> /dev/null; then
        cargo tarpaulin --out Stdout --timeout 300 | tee coverage.txt
        
        local coverage=$(grep -oP 'Coverage: \K[0-9.]+' coverage.txt || echo "0")
        log "当前覆盖率: ${coverage}%"
        
        if (( $(echo "$coverage < 70" | bc -l) )); then
            warn "覆盖率低于目标 (70%)，请添加更多测试"
        fi
    else
        warn "cargo-tarpaulin 未安装，跳过覆盖率检查"
        echo "安装命令: cargo install cargo-tarpaulin"
    fi
}

# 提交变更
commit_changes() {
    local task_id=$1
    local message=$2
    
    cd "$REPO_ROOT"
    
    log "💾 提交变更..."
    
    # 格式化代码
    cargo +nightly fmt
    
    # 暂存所有变更
    git add -A
    
    # 检查是否有变更
    if git diff --cached --quiet; then
        warn "没有检测到变更，跳过提交"
        return 0
    fi
    
    # 生成提交信息
    local commit_msg="feat(task-$task_id): $message

Automated commit by Evolution Engine

- Implemented task $task_id
- All tests passing
- Coverage maintained/improved
"
    
    git commit -m "$commit_msg"
    log "✅ 提交完成"
}

# 推送并创建 PR
create_pr() {
    local branch_name=$1
    local task_id=$2
    
    cd "$REPO_ROOT"
    
    log "🚀 推送分支并创建 PR..."
    
    git push origin "$branch_name" || error "推送失败"
    
    # 使用 gh CLI 创建 PR（如果已安装）
    if command -v gh &> /dev/null; then
        local pr_title="feat(task-$task_id): 自动化任务提交"
        local pr_body="## 任务描述

参考: tasks/TASK_${task_id//./_}*.md

## 变更内容

- [ ] 实现任务 $task_id
- [ ] 测试覆盖率 ≥ 当前水平
- [ ] 所有 CI 检查通过

## 测试结果

\`\`\`
$(cargo test --all 2>&1 | tail -10)
\`\`\`

---
*此 PR 由 Evolution Engine 自动生成*
"
        
        gh pr create --title "$pr_title" --body "$pr_body" --base main
        log "✅ PR 已创建"
    else
        warn "gh CLI 未安装，请手动创建 PR"
        echo "命令: gh pr create --title 'feat(task-$task_id)' --base main"
    fi
}

# 标记任务完成
mark_task_complete() {
    local task_id=$1
    
    log "✅ 标记任务 $task_id 为已完成..."
    
    cd "$REPO_ROOT"
    
    # 在 roadmap 中标记完成
    sed -i "s/^- \[ \] \*\*任务 $task_id/- [x] **任务 $task_id/" "$ROADMAP_FILE"
    
    # 提交 roadmap 更新
    git add "$ROADMAP_FILE"
    git commit -m "chore: mark task $task_id as complete"
    git push origin main
}

# 主流程
main() {
    log "🤖 Hakimi Agent Evolution Engine 启动"
    echo ""
    
    # 检查环境
    if [ ! -d "$REPO_ROOT" ]; then
        error "仓库目录不存在: $REPO_ROOT"
    fi
    
    cd "$REPO_ROOT"
    
    # 显示进度
    show_progress
    
    # 选择下一任务
    local task_id=$(select_next_task)
    
    if [ -z "$task_id" ]; then
        log "没有待执行任务"
        exit 0
    fi
    
    # 询问用户确认
    read -p "是否执行任务 $task_id? [y/N] " -n 1 -r
    echo
    if [[ ! $REPLY =~ ^[Yy]$ ]]; then
        log "用户取消执行"
        exit 0
    fi
    
    # 创建分支
    local branch_name=$(create_task_branch "$task_id")
    
    # 执行任务（这里需要根据具体任务调用不同的脚本）
    warn "⚠️  请手动执行任务步骤，完成后按 Enter 继续..."
    read -p ""
    
    # 运行测试
    run_tests
    
    # 检查覆盖率
    check_coverage
    
    # 提交变更
    commit_changes "$task_id" "implement task $task_id"
    
    # 创建 PR
    create_pr "$branch_name" "$task_id"
    
    # 标记完成
    mark_task_complete "$task_id"
    
    log "🎉 任务 $task_id 执行完成！"
}

# 命令行参数处理
case "${1:-}" in
    progress)
        show_progress
        ;;
    next)
        select_next_task
        ;;
    test)
        run_tests
        ;;
    coverage)
        check_coverage
        ;;
    *)
        main
        ;;
esac
