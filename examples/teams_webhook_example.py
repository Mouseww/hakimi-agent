#!/usr/bin/env python3
"""
Microsoft Teams Webhook 集成示例 — Python + FastAPI
完整实现双向通道（Outgoing Webhook + Power Automate Workflows）

依赖安装：
    pip install fastapi uvicorn httpx anthropic

运行：
    export TEAMS_HMAC_SECRET="<base64_string>"
    export DEFAULT_WORKFLOW_URL="https://prod-xx.westus.logic.azure.com/..."
    export ANTHROPIC_API_KEY="sk-ant-..."
    python teams_webhook_example.py

部署到 Azure App Service：
    # requirements.txt
    fastapi
    uvicorn
    httpx
    anthropic
    
    # 启动命令（App Service 配置 -> 常规设置 -> 启动命令）
    gunicorn -k uvicorn.workers.UvicornWorker -w 2 -b 0.0.0.0:8000 teams_webhook_example:app
"""

import base64
import hashlib
import hmac
import json
import os
import re
import asyncio
from typing import Optional

import httpx
from fastapi import FastAPI, Request, Response
from pydantic import BaseModel

# ═══════════════════════════════════════════════════════════════════════════
# 配置（从环境变量读取）
# ═══════════════════════════════════════════════════════════════════════════

HMAC_SECRET = os.environ["TEAMS_HMAC_SECRET"]  # Outgoing Webhook 创建时的安全令牌（base64）
DEFAULT_WORKFLOW_URL = os.environ.get("DEFAULT_WORKFLOW_URL", "")  # 兜底推送地址

# 频道ID -> Workflows URL 映射（JSON 字符串）
# 例: {"19:abc...@thread.tacv2": "https://prod-xx.westus.logic.azure.com/..."}
CHANNEL_WORKFLOWS = json.loads(os.environ.get("CHANNEL_WORKFLOWS", "{}"))

# ═══════════════════════════════════════════════════════════════════════════
# FastAPI App
# ═══════════════════════════════════════════════════════════════════════════

app = FastAPI(title="Teams Webhook Integration")

# ═══════════════════════════════════════════════════════════════════════════
# HMAC 签名验证
# ═══════════════════════════════════════════════════════════════════════════

def verify_hmac(raw_body: bytes, auth_header: str) -> bool:
    """验证 Teams Outgoing Webhook 的 HMAC 签名。
    
    Teams 在 Authorization 头里带 'HMAC <base64签名>'，
    签名 = HMAC-SHA256(key=base64解码后的密钥, msg=原始请求体) 再 base64。
    """
    if not auth_header or not auth_header.startswith("HMAC "):
        return False
    
    provided = auth_header[len("HMAC "):]
    
    # 解码密钥
    key = base64.b64decode(HMAC_SECRET)
    
    # 计算签名
    digest = hmac.new(key, raw_body, hashlib.sha256).digest()
    expected = base64.b64encode(digest).decode()
    
    # 常量时间比较
    return hmac.compare_digest(provided, expected)


# ═══════════════════════════════════════════════════════════════════════════
# 入方向端点（接收 Teams 消息）
# ═══════════════════════════════════════════════════════════════════════════

class TeamsActivity(BaseModel):
    """Teams Outgoing Webhook 发来的 Activity"""
    type: str
    id: Optional[str] = None
    timestamp: Optional[str] = None
    from_: Optional[dict] = None  # {"id": "...", "name": "张三", "aadObjectId": "..."}
    text: Optional[str] = None
    channelData: Optional[dict] = None  # {"teamsChannelId": "19:xxx@thread.tacv2", ...}
    
    class Config:
        fields = {"from_": "from"}


@app.post("/teams/inbound")
async def teams_inbound(request: Request):
    """Teams Outgoing Webhook 入口"""
    raw_body = await request.body()
    
    # 验证 HMAC 签名
    auth_header = request.headers.get("Authorization", "")
    if not verify_hmac(raw_body, auth_header):
        return Response(status_code=401, content="Invalid signature")
    
    # 解析 Activity
    activity = TeamsActivity.parse_raw(raw_body)
    
    # 提取正文（去除 HTML 标签和 @提及）
    text = re.sub(r"<[^>]+>", "", activity.text or "").strip()
    
    if not text:
        return {"type": "message", "text": "Message received but appears to be empty."}
    
    user_name = (activity.from_ or {}).get("name", "Unknown User")
    channel_id = (activity.channelData or {}).get("teamsChannelId", "")
    
    # 异步拉起智能体（不阻塞回执）
    asyncio.create_task(run_agent(text, user_name, channel_id))
    
    # 立即返回回执（必须 10 秒内）
    return {
        "type": "message",
        "text": f"收到，{user_name}。任务已开始处理，结果稍后发到本频道。"
    }


# ═══════════════════════════════════════════════════════════════════════════
# 智能体执行（替换成你的逻辑）
# ═══════════════════════════════════════════════════════════════════════════

async def run_agent(task_text: str, user_name: str, channel_id: str):
    """异步执行智能体任务并推送结果"""
    try:
        # 调用你的智能体（这里用 Claude API 作为示例）
        result = await call_claude(task_text)
        
        # 推送成功结果
        await push_to_channel(
            channel_id,
            title=f"任务完成: {task_text[:40]}",
            body=result,
            requester=user_name
        )
    except Exception as e:
        # 推送错误通知
        await push_to_channel(
            channel_id,
            title="任务失败",
            body=f"错误: {e}",
            requester=user_name
        )


async def call_claude(task_text: str) -> str:
    """最小示例: 直接调 Claude API
    
    后续接入 Jira/Bitbucket 时，把这里替换为你的编排逻辑，
    或改为触发一个运行 Claude Code headless 的容器任务。
    """
    from anthropic import AsyncAnthropic
    
    client = AsyncAnthropic()  # 读 ANTHROPIC_API_KEY 环境变量
    msg = await client.messages.create(
        model="claude-sonnet-4-20250514",
        max_tokens=2000,
        messages=[{"role": "user", "content": task_text}],
    )
    return "".join(b.text for b in msg.content if b.type == "text")


# ═══════════════════════════════════════════════════════════════════════════
# 出方向推送（Adaptive Card）
# ═══════════════════════════════════════════════════════════════════════════

async def push_to_channel(channel_id: str, title: str, body: str, requester: str):
    """推送 Adaptive Card 到指定频道的 Workflows webhook"""
    # 获取该频道的 Workflows URL
    url = CHANNEL_WORKFLOWS.get(channel_id, DEFAULT_WORKFLOW_URL)
    if not url:
        print(f"[warn] 频道 {channel_id} 没有配置 Workflows URL，消息丢弃")
        return
    
    # 构建 Adaptive Card
    card = {
        "type": "message",
        "attachments": [{
            "contentType": "application/vnd.microsoft.card.adaptive",
            "content": {
                "$schema": "http://adaptivecards.io/schemas/adaptive-card.json",
                "type": "AdaptiveCard",
                "version": "1.4",
                "body": [
                    {
                        "type": "TextBlock",
                        "size": "Medium",
                        "weight": "Bolder",
                        "text": title
                    },
                    {
                        "type": "TextBlock",
                        "text": f"发起人: {requester}",
                        "isSubtle": True,
                        "spacing": "None"
                    },
                    {
                        "type": "TextBlock",
                        "text": body[:6000],  # Teams 限制
                        "wrap": True
                    }
                ]
            }
        }]
    }
    
    # POST 到 Workflows webhook
    async with httpx.AsyncClient(timeout=30) as client:
        r = await client.post(url, json=card)
        if r.status_code >= 300:
            print(f"[error] 推送失败 {r.status_code}: {r.text[:200]}")


# ═══════════════════════════════════════════════════════════════════════════
# 带按钮的进阶卡片示例
# ═══════════════════════════════════════════════════════════════════════════

async def push_with_buttons(channel_id: str, title: str, facts: dict, actions: list):
    """推送带按钮的 Adaptive Card
    
    Args:
        channel_id: Teams 频道 ID
        title: 卡片标题
        facts: 事实列表，例如 {"测试通过率": "48/50", "环境": "staging"}
        actions: 按钮列表，例如 [{"title": "查看 Jira", "url": "https://..."}]
    """
    url = CHANNEL_WORKFLOWS.get(channel_id, DEFAULT_WORKFLOW_URL)
    if not url:
        return
    
    card = {
        "type": "message",
        "attachments": [{
            "contentType": "application/vnd.microsoft.card.adaptive",
            "content": {
                "$schema": "http://adaptivecards.io/schemas/adaptive-card.json",
                "type": "AdaptiveCard",
                "version": "1.4",
                "body": [
                    {"type": "TextBlock", "weight": "Bolder", "size": "Medium", "text": title},
                    {
                        "type": "FactSet",
                        "facts": [{"title": k, "value": v} for k, v in facts.items()]
                    }
                ],
                "actions": [
                    {"type": "Action.OpenUrl", "title": a["title"], "url": a["url"]}
                    for a in actions
                ]
            }
        }]
    }
    
    async with httpx.AsyncClient(timeout=30) as client:
        await client.post(url, json=card)


# ═══════════════════════════════════════════════════════════════════════════
# Health Check
# ═══════════════════════════════════════════════════════════════════════════

@app.get("/healthz")
async def healthz():
    return {"ok": True}


# ═══════════════════════════════════════════════════════════════════════════
# 本地运行
# ═══════════════════════════════════════════════════════════════════════════

if __name__ == "__main__":
    import uvicorn
    uvicorn.run(app, host="0.0.0.0", port=8000)
