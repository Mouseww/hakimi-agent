# Example Plugin

这是一个简单的 Hakimi Agent 插件示例，展示了插件的基本结构。

## 功能

- 导出插件元数据
- 最小化实现，作为模板使用

## 构建

```bash
cargo build --release
```

生成的动态库位于 `target/release/`:

- Linux: `libexample_plugin.so`
- macOS: `libexample_plugin.dylib`
- Windows: `example_plugin.dll`

## 安装

```bash
mkdir -p ~/.hakimi/plugins
cp target/release/libexample_plugin.so ~/.hakimi/plugins/
```

## 配置

在 `~/.hakimi/plugins.yaml` 中添加：

```yaml
plugins:
  - id: example_plugin
    enabled: true
```

## 测试

```bash
cargo test
```

## 扩展

要扩展此插件，可以：

1. 实现 `HakimiPlugin` trait（TASK 4.1.1）
2. 添加钩子函数（消息、工具、会话）
3. 集成外部服务或库

参考 `docs/plugin_development_guide.md` 获取详细指南。

## 许可证

MIT
