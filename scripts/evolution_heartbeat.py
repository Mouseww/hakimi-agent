#!/usr/bin/env python3
"""
Hakimi Evolution Engine - Autonomous Task Executor
持续自动推进 EVOLUTION_ROADMAP.md 中的任务
"""
import os
import sys
import json
import subprocess
from datetime import datetime
from pathlib import Path

REPO_ROOT = Path("/root/hakimi-agent")
TASKS_DIR = REPO_ROOT / "tasks"
STATE_FILE = REPO_ROOT / ".evolution_state.json"

def load_state():
    """加载上次执行状态"""
    if STATE_FILE.exists():
        return json.loads(STATE_FILE.read_text())
    return {"current_task": None, "completed": [], "failed": []}

def save_state(state):
    """保存执行状态"""
    STATE_FILE.write_text(json.dumps(state, indent=2))

def get_next_task():
    """根据 EVOLUTION_ROADMAP 获取下一个待执行任务"""
    roadmap = REPO_ROOT / "EVOLUTION_ROADMAP.md"
    if not roadmap.exists():
        return None
    
    content = roadmap.read_text()
    state = load_state()
    completed = set(state.get("completed", []))
    
    # Phase 1 任务优先级队列
    phase1_tasks = [
        "TASK_1.1.2_performance_metrics.md",
        "TASK_1.1.3_error_types.md",
        "TASK_1.2.1_working_memory_cleanup.md",
        "TASK_1.2.2_memory_capacity_monitor.md",
        "TASK_1.2.3_memory_archive.md",
        "TASK_1.3.1_session_search_integration_test.md",
        "TASK_1.3.2_memory_error_path_test.md",
        "TASK_1.3.3_stress_test.md",
    ]
    
    for task_file in phase1_tasks:
        if task_file not in completed:
            task_path = TASKS_DIR / task_file
            if task_path.exists():
                # 检查任务状态
                content = task_path.read_text()
                if "✅ 已完成" not in content and "🔴 已完成" not in content:
                    return task_file
    
    return None

def execute_task(task_file):
    """执行单个任务"""
    print(f"\n{'='*60}")
    print(f"🚀 开始执行任务: {task_file}")
    print(f"{'='*60}\n")
    
    task_name = task_file.replace("TASK_", "").replace(".md", "").replace("_", " ").title()
    
    # 读取任务文件获取详细信息
    task_path = TASKS_DIR / task_file
    if not task_path.exists():
        print(f"❌ 任务文件不存在: {task_path}")
        return False
    
    task_content = task_path.read_text()
    
    # 提取关键信息
    print(f"📋 任务: {task_name}")
    print(f"📄 文件: {task_file}")
    
    # 创建特性分支
    branch_name = f"feat/{task_file.replace('TASK_', '').replace('.md', '').replace('_', '-')}"
    
    try:
        subprocess.run(["git", "checkout", "-b", branch_name], cwd=REPO_ROOT, check=True)
        print(f"✅ 创建分支: {branch_name}\n")
    except subprocess.CalledProcessError:
        # 分支可能已存在
        subprocess.run(["git", "checkout", branch_name], cwd=REPO_ROOT, check=False)
    
    # 返回任务信息供 agent 处理
    return {
        "task_file": task_file,
        "task_name": task_name,
        "branch": branch_name,
        "content": task_content,
        "repo_root": str(REPO_ROOT)
    }

def main():
    """主循环"""
    state = load_state()
    
    # 获取下一个任务
    next_task = get_next_task()
    
    if not next_task:
        print("\n🎉 Phase 1 所有任务已完成！")
        print("📊 运行 `./scripts/evolution_engine.sh progress` 查看整体进度")
        return 0
    
    # 执行任务
    task_info = execute_task(next_task)
    
    if task_info:
        # 输出 JSON 供 Hermes 解析
        print("\n" + "="*60)
        print("📦 TASK_INFO_JSON_START")
        print(json.dumps(task_info, indent=2, ensure_ascii=False))
        print("📦 TASK_INFO_JSON_END")
        print("="*60 + "\n")
        return 0
    else:
        state["failed"].append(next_task)
        save_state(state)
        return 1

if __name__ == "__main__":
    sys.exit(main())
