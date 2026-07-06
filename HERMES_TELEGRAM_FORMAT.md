# Hermes Telegram Markdown 处理方案

## 核心策略

1. **使用 MarkdownV2**（不是 legacy Markdown）
2. **占位符保护策略**：先提取保护区域 → 转换语法 → 转义 → 恢复
3. **表格 → Bullet 列表**（Telegram 不支持表格）
4. **安全网**：最后检查代码块外的裸括号

## 转换流程

### 0. 表格重写
```python
_wrap_markdown_tables(text)  # | col1 | col2 | → **Row 1**\n• col1: val1\n• col2: val2
```

### 1-2. 保护代码块和行内代码
```python
# 代码块
```python\ncode```  →  占位符PH0  (body内 \\ → \\\\, ` → \\`)

# 行内代码
`code`  →  占位符PH1  (内部 \\ → \\\\)
```

### 3. 链接转换
```python
[text](url)  →  占位符PH2 = [escaped_text](escaped_url)
# text: 完整转义
# url: 只转义 \\ 和 )
```

### 4-9. Markdown → MarkdownV2
```python
# 标题 → 加粗
## Title  →  占位符PH3 = *Title*

# 粗体
**text**  →  占位符PH4 = *text*  (MDV2 bold)

# 斜体
*text*  →  占位符PH5 = _text_  (MDV2 italic)

# 删除线
~~text~~  →  占位符PH6 = ~text~

# 剧透
||text||  →  占位符PH7 = ||text||

# 引用
> text  →  占位符PH8 = > text
```

### 10. 转义所有剩余特殊字符
```python
_escape_mdv2(text)  # 转义 _*[]()~`>#+-=|{}.!\
```

### 11. 恢复占位符
```python
for key in reversed(placeholders.keys()):
    text = text.replace(key, placeholders[key])
```

### 12. 安全网
```python
# 分离代码/非代码区域
_code_split = re.split(r'(```[\s\S]*?```|`[^`]+`)', text)
# 只在非代码区域转义裸 () {}
```

## 关键正则表达式

```python
# MarkdownV2 需要转义的字符
_MDV2_ESCAPE_RE = re.compile(r'([_*\[\]()~`>#\+\-=|{}.!\\])')

# 代码块
r'(```(?:[^\n]*\n)?[\s\S]*?```)'

# 行内代码
r'(`[^`]+`)'

# 链接
r'\[([^\]]+)\]\(([^()]*(?:\([^()]*\)[^()]*)*)\)'

# 标题
r'^#{1,6}\s+(.+)$'  (MULTILINE)

# 粗体
r'\*\*(.+?)\*\*'

# 斜体
r'\*([^*\n]+)\*'

# 删除线
r'~~(.+?)~~'

# 剧透
r'\|\|(.+?)\|\|'

# 引用
r'^((?:\*\*)?>{1,3}) (.+)$'  (MULTILINE)
```

## 表格处理

```python
# 检测分隔行
_TABLE_SEPARATOR_RE = r'^\s*\|?\s*:?-+:?\s*(?:\|\s*:?-+:?\s*){1,}\|?\s*$'

# 重写为
**Row 1**
• col1: val1
• col2: val2

**Row 2**
• col1: val3
• col2: val4
```

## Hakimi 实现任务清单

- [ ] 修改 `sanitize_for_markdown` → `format_message_mdv2`
- [ ] 实现占位符保护系统
- [ ] 转换标准 Markdown → MarkdownV2 语法
- [ ] 实现表格重写逻辑
- [ ] 安全网：最后检查裸括号
- [ ] 修改所有 `ParseMode::Markdown` → `ParseMode::MARKDOWN_V2`
- [ ] 测试完整的 Markdown 格式（标题/粗体/斜体/代码/链接/表格）
