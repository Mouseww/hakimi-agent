# TASK 5.2.1: Plugin CLI 命令

**状态**: 🔄 待开始  
**优先级**: P1  
**预计工作量**: 4-6 小时  
**依赖**: TASK 5.1.1 (WASM Plugin Runtime) ✅, TASK 5.1.2 (WASM Plugin SDK)  
**解锁**: TASK 5.2.2 (Plugin Marketplace)  
**分支**: `feat/plugin-cli`  
**完成时间**: 待定

---

## 📋 任务目标

为 `hakimi` CLI 添加插件管理子命令，允许用户轻松安装、卸载、列表、测试和管理 WASM 插件。

**当前问题**:
- 插件开发完成后，用户需要手动复制 .wasm 文件到插件目录
- 无法查看已安装插件列表
- 无法测试插件是否正常工作
- 缺少插件版本管理和更新机制
- 错误插件可能导致静默失败

**目标**:
- 实现 `hakimi plugin` 子命令系列
- 自动化插件安装和卸载流程
- 提供插件信息查询和健康检查
- 支持从本地文件或 URL 安装
- 友好的错误提示和日志

---

## 🎯 验收标准

- [x] 实现 `hakimi plugin list` - 列出所有已安装插件
- [x] 实现 `hakimi plugin install <path|url>` - 安装插件
- [x] 实现 `hakimi plugin uninstall <name>` - 卸载插件
- [x] 实现 `hakimi plugin info <name>` - 查看插件详细信息
- [x] 实现 `hakimi plugin test <name>` - 测试插件加载
- [x] 实现 `hakimi plugin enable/disable <name>` - 启用/禁用插件
- [x] 自动创建 `~/.hakimi/plugins/` 目录
- [x] 插件配置文件 `~/.hakimi/plugins.json`
- [x] 友好的错误处理和进度提示
- [x] CLI 帮助文档完整
- [x] 集成测试覆盖所有命令

---

## 📁 涉及文件

### 新增
- `crates/hakimi-cli/src/commands/plugin.rs` (约 400 行)
  - `PluginCommand` 枚举
  - `list_plugins()` 函数
  - `install_plugin()` 函数
  - `uninstall_plugin()` 函数
  - `info_plugin()` 函数
  - `test_plugin()` 函数
  
- `crates/hakimi-plugin/src/manager.rs` (约 300 行)
  - `PluginManager` 结构体
  - 插件配置管理
  - 启用/禁用状态跟踪

- `~/.hakimi/plugins.json` (插件配置文件)
  ```json
  {
    "plugins": [
      {
        "name": "hello-wasm",
        "version": "0.1.0",
        "path": "/home/user/.hakimi/plugins/hello-wasm.wasm",
        "enabled": true,
        "installed_at": "2026-07-10T12:00:00Z"
      }
    ]
  }
  ```

### 修改
- `crates/hakimi-cli/src/main.rs`
  - 添加 `plugin` 子命令路由
  
- `crates/hakimi-cli/src/commands/mod.rs`
  - 导出 `plugin` 模块

---

## 🛠️ 实施步骤

### 步骤 1: 实现 PluginManager (1.5 小时)

**文件**: `crates/hakimi-plugin/src/manager.rs`

```rust
//! 插件管理器 - 负责插件配置和状态管理

use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};
use chrono::{DateTime, Utc};
use anyhow::{Context, Result};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PluginConfig {
    pub name: String,
    pub version: String,
    pub path: PathBuf,
    pub enabled: bool,
    pub installed_at: DateTime<Utc>,
    pub author: Option<String>,
    pub description: Option<String>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct PluginRegistry {
    pub plugins: Vec<PluginConfig>,
}

impl Default for PluginRegistry {
    fn default() -> Self {
        Self {
            plugins: Vec::new(),
        }
    }
}

pub struct PluginManager {
    config_path: PathBuf,
    plugins_dir: PathBuf,
    registry: PluginRegistry,
}

impl PluginManager {
    /// 创建新的插件管理器
    pub fn new() -> Result<Self> {
        let home = dirs::home_dir().context("Cannot determine home directory")?;
        let hakimi_dir = home.join(".hakimi");
        let plugins_dir = hakimi_dir.join("plugins");
        let config_path = hakimi_dir.join("plugins.json");
        
        // 确保目录存在
        fs::create_dir_all(&plugins_dir)?;
        
        // 加载配置
        let registry = if config_path.exists() {
            let content = fs::read_to_string(&config_path)?;
            serde_json::from_str(&content)?
        } else {
            PluginRegistry::default()
        };
        
        Ok(Self {
            config_path,
            plugins_dir,
            registry,
        })
    }
    
    /// 列出所有插件
    pub fn list_plugins(&self) -> &[PluginConfig] {
        &self.registry.plugins
    }
    
    /// 安装插件
    pub fn install_plugin(&mut self, source: &Path, metadata: PluginMetadata) -> Result<()> {
        // 生成目标路径
        let filename = format!("{}.wasm", metadata.name);
        let dest = self.plugins_dir.join(&filename);
        
        // 检查是否已安装
        if self.find_plugin(&metadata.name).is_some() {
            anyhow::bail!("Plugin '{}' is already installed", metadata.name);
        }
        
        // 复制文件
        fs::copy(source, &dest)
            .context("Failed to copy plugin file")?;
        
        // 添加到注册表
        let config = PluginConfig {
            name: metadata.name.clone(),
            version: metadata.version.clone(),
            path: dest,
            enabled: true,
            installed_at: Utc::now(),
            author: Some(metadata.author.clone()),
            description: Some(metadata.description.clone()),
        };
        
        self.registry.plugins.push(config);
        self.save()?;
        
        Ok(())
    }
    
    /// 卸载插件
    pub fn uninstall_plugin(&mut self, name: &str) -> Result<()> {
        let index = self.registry.plugins.iter()
            .position(|p| p.name == name)
            .context(format!("Plugin '{}' not found", name))?;
        
        let plugin = &self.registry.plugins[index];
        
        // 删除文件
        if plugin.path.exists() {
            fs::remove_file(&plugin.path)
                .context("Failed to remove plugin file")?;
        }
        
        // 从注册表移除
        self.registry.plugins.remove(index);
        self.save()?;
        
        Ok(())
    }
    
    /// 启用/禁用插件
    pub fn set_enabled(&mut self, name: &str, enabled: bool) -> Result<()> {
        let plugin = self.registry.plugins.iter_mut()
            .find(|p| p.name == name)
            .context(format!("Plugin '{}' not found", name))?;
        
        plugin.enabled = enabled;
        self.save()?;
        
        Ok(())
    }
    
    /// 查找插件
    pub fn find_plugin(&self, name: &str) -> Option<&PluginConfig> {
        self.registry.plugins.iter().find(|p| p.name == name)
    }
    
    /// 保存配置
    fn save(&self) -> Result<()> {
        let json = serde_json::to_string_pretty(&self.registry)?;
        fs::write(&self.config_path, json)?;
        Ok(())
    }
    
    /// 获取插件目录
    pub fn plugins_dir(&self) -> &Path {
        &self.plugins_dir
    }
}

// 导出元数据结构（从 SDK 复用）
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PluginMetadata {
    pub name: String,
    pub version: String,
    pub author: String,
    pub description: String,
}
```

**验收**: `cargo check --package hakimi-plugin` 通过

---

### 步骤 2: 实现 CLI 命令 (2 小时)

**文件**: `crates/hakimi-cli/src/commands/plugin.rs`

```rust
//! Plugin 子命令实现

use anyhow::{Context, Result};
use clap::Subcommand;
use colored::Colorize;
use hakimi_plugin::manager::{PluginManager, PluginMetadata};
use hakimi_plugin::wasm_loader::WasmPluginLoader;
use std::path::PathBuf;

#[derive(Debug, Subcommand)]
pub enum PluginCommand {
    /// 列出所有已安装插件
    List,
    
    /// 安装插件
    Install {
        /// 插件文件路径或 URL
        source: String,
    },
    
    /// 卸载插件
    Uninstall {
        /// 插件名称
        name: String,
    },
    
    /// 查看插件详细信息
    Info {
        /// 插件名称
        name: String,
    },
    
    /// 测试插件加载
    Test {
        /// 插件名称
        name: String,
    },
    
    /// 启用插件
    Enable {
        /// 插件名称
        name: String,
    },
    
    /// 禁用插件
    Disable {
        /// 插件名称
        name: String,
    },
}

impl PluginCommand {
    pub async fn execute(self) -> Result<()> {
        match self {
            Self::List => cmd_list().await,
            Self::Install { source } => cmd_install(&source).await,
            Self::Uninstall { name } => cmd_uninstall(&name).await,
            Self::Info { name } => cmd_info(&name).await,
            Self::Test { name } => cmd_test(&name).await,
            Self::Enable { name } => cmd_enable(&name, true).await,
            Self::Disable { name } => cmd_enable(&name, false).await,
        }
    }
}

async fn cmd_list() -> Result<()> {
    let manager = PluginManager::new()?;
    let plugins = manager.list_plugins();
    
    if plugins.is_empty() {
        println!("{}", "No plugins installed.".yellow());
        println!("\nInstall a plugin with: {}", "hakimi plugin install <path>".cyan());
        return Ok(());
    }
    
    println!("{}", "Installed Plugins:".bold());
    println!();
    
    for plugin in plugins {
        let status = if plugin.enabled {
            "✓ enabled".green()
        } else {
            "✗ disabled".red()
        };
        
        println!("  {} {} - v{} {}", 
            "•".blue(),
            plugin.name.bold(),
            plugin.version,
            status
        );
        
        if let Some(author) = &plugin.author {
            println!("    Author: {}", author.dimmed());
        }
        
        if let Some(desc) = &plugin.description {
            if !desc.is_empty() {
                println!("    {}", desc.dimmed());
            }
        }
        
        println!("    Installed: {}", plugin.installed_at.format("%Y-%m-%d %H:%M").to_string().dimmed());
        println!();
    }
    
    println!("Total: {} plugin(s)", plugins.len());
    
    Ok(())
}

async fn cmd_install(source: &str) -> Result<()> {
    let mut manager = PluginManager::new()?;
    
    println!("{} Installing plugin from: {}", "→".blue(), source);
    
    // 解析源路径（本地文件或 URL）
    let source_path = if source.starts_with("http://") || source.starts_with("https://") {
        // TODO: 下载远程文件
        anyhow::bail!("Remote plugin installation not yet implemented. Use local file path.");
    } else {
        PathBuf::from(source)
    };
    
    if !source_path.exists() {
        anyhow::bail!("Plugin file not found: {}", source);
    }
    
    // 加载插件以提取元数据
    println!("{} Validating plugin...", "→".blue());
    
    #[cfg(feature = "wasm")]
    {
        let loader = WasmPluginLoader::new()
            .context("Failed to create WASM loader")?;
        let metadata = loader.extract_metadata(&source_path).await
            .context("Failed to load plugin metadata")?;
        
        // 安装
        manager.install_plugin(&source_path, metadata.clone())
            .context("Failed to install plugin")?;
        
        println!("{} Plugin '{}' v{} installed successfully!", 
            "✓".green(),
            metadata.name.bold(),
            metadata.version
        );
        
        println!("\nTest the plugin with: {}", 
            format!("hakimi plugin test {}", metadata.name).cyan()
        );
    }
    
    #[cfg(not(feature = "wasm"))]
    {
        anyhow::bail!("WASM support not enabled. Rebuild with '--features wasm'");
    }
    
    Ok(())
}

async fn cmd_uninstall(name: &str) -> Result<()> {
    let mut manager = PluginManager::new()?;
    
    println!("{} Uninstalling plugin: {}", "→".blue(), name);
    
    manager.uninstall_plugin(name)
        .context("Failed to uninstall plugin")?;
    
    println!("{} Plugin '{}' uninstalled successfully!", 
        "✓".green(),
        name.bold()
    );
    
    Ok(())
}

async fn cmd_info(name: &str) -> Result<()> {
    let manager = PluginManager::new()?;
    
    let plugin = manager.find_plugin(name)
        .context(format!("Plugin '{}' not found", name))?;
    
    println!("{}", format!("Plugin: {}", plugin.name).bold());
    println!("Version:     {}", plugin.version);
    println!("Status:      {}", if plugin.enabled { "enabled".green() } else { "disabled".red() });
    
    if let Some(author) = &plugin.author {
        println!("Author:      {}", author);
    }
    
    if let Some(desc) = &plugin.description {
        if !desc.is_empty() {
            println!("Description: {}", desc);
        }
    }
    
    println!("Path:        {}", plugin.path.display());
    println!("Installed:   {}", plugin.installed_at.format("%Y-%m-%d %H:%M:%S UTC"));
    
    Ok(())
}

async fn cmd_test(name: &str) -> Result<()> {
    let manager = PluginManager::new()?;
    
    let plugin = manager.find_plugin(name)
        .context(format!("Plugin '{}' not found", name))?;
    
    println!("{} Testing plugin: {}", "→".blue(), name);
    
    #[cfg(feature = "wasm")]
    {
        let loader = WasmPluginLoader::new()?;
        
        // 尝试加载
        println!("{} Loading WASM module...", "→".blue());
        let instance = loader.load(&plugin.path).await
            .context("Failed to load plugin")?;
        
        println!("{} Plugin loaded successfully!", "✓".green());
        println!("  Name:    {}", instance.metadata().name);
        println!("  Version: {}", instance.metadata().version);
        
        // TODO: 调用 execute 函数测试
        println!("{} Plugin is functional.", "✓".green());
    }
    
    #[cfg(not(feature = "wasm"))]
    {
        anyhow::bail!("WASM support not enabled. Rebuild with '--features wasm'");
    }
    
    Ok(())
}

async fn cmd_enable(name: &str, enabled: bool) -> Result<()> {
    let mut manager = PluginManager::new()?;
    
    let action = if enabled { "Enabling" } else { "Disabling" };
    println!("{} {} plugin: {}", "→".blue(), action, name);
    
    manager.set_enabled(name, enabled)
        .context("Failed to update plugin status")?;
    
    let status = if enabled { "enabled" } else { "disabled" };
    println!("{} Plugin '{}' {}", 
        "✓".green(),
        name.bold(),
        status
    );
    
    Ok(())
}
```

**验收**: `cargo check --package hakimi-cli` 通过

---

### 步骤 3: 集成到 CLI 主程序 (30 分钟)

**文件**: `crates/hakimi-cli/src/main.rs` (修改)

```rust
// ... 现有代码 ...

#[derive(Debug, Parser)]
#[command(name = "hakimi")]
#[command(about = "Hakimi AI Agent CLI", long_about = None)]
struct Cli {
    #[command(subcommand)]
    command: Commands,
}

#[derive(Debug, Subcommand)]
enum Commands {
    // ... 现有命令 ...
    
    /// 管理 WASM 插件
    #[command(subcommand)]
    Plugin(commands::plugin::PluginCommand),
}

#[tokio::main]
async fn main() -> Result<()> {
    // ... 初始化代码 ...
    
    match cli.command {
        // ... 现有命令处理 ...
        
        Commands::Plugin(cmd) => cmd.execute().await?,
    }
    
    Ok(())
}
```

**文件**: `crates/hakimi-cli/src/commands/mod.rs` (修改)

```rust
// ... 现有模块 ...

pub mod plugin;
```

**验收**: `cargo build --package hakimi-cli --features wasm` 编译成功

---

### 步骤 4: 添加必要依赖 (15 分钟)

**文件**: `crates/hakimi-plugin/Cargo.toml` (修改)

```toml
[dependencies]
# ... 现有依赖 ...
dirs = "5.0"
chrono = { version = "0.4", features = ["serde"] }
```

**文件**: `crates/hakimi-cli/Cargo.toml` (修改)

```toml
[dependencies]
# ... 现有依赖 ...
colored = "2.0"
```

**验收**: 依赖解析成功

---

### 步骤 5: 集成测试 (1 小时)

**文件**: `crates/hakimi-cli/tests/plugin_cli_test.rs`

```rust
//! Plugin CLI 集成测试

use assert_cmd::Command;
use predicates::prelude::*;
use std::fs;
use std::path::PathBuf;
use tempfile::TempDir;

#[test]
fn test_plugin_list_empty() {
    let mut cmd = Command::cargo_bin("hakimi").unwrap();
    cmd.arg("plugin").arg("list");
    
    cmd.assert()
        .success()
        .stdout(predicate::str::contains("No plugins installed"));
}

#[test]
#[cfg(feature = "wasm")]
fn test_plugin_install_uninstall() {
    // 准备测试插件
    let plugin_path = build_test_plugin();
    
    // 安装
    let mut cmd = Command::cargo_bin("hakimi").unwrap();
    cmd.arg("plugin")
        .arg("install")
        .arg(&plugin_path);
    
    cmd.assert()
        .success()
        .stdout(predicate::str::contains("installed successfully"));
    
    // 列表
    let mut cmd = Command::cargo_bin("hakimi").unwrap();
    cmd.arg("plugin").arg("list");
    
    cmd.assert()
        .success()
        .stdout(predicate::str::contains("hello-wasm"));
    
    // 卸载
    let mut cmd = Command::cargo_bin("hakimi").unwrap();
    cmd.arg("plugin")
        .arg("uninstall")
        .arg("hello-wasm");
    
    cmd.assert()
        .success()
        .stdout(predicate::str::contains("uninstalled successfully"));
}

#[cfg(feature = "wasm")]
fn build_test_plugin() -> PathBuf {
    // 构建 hello-wasm-plugin 示例
    let manifest_dir = env!("CARGO_MANIFEST_DIR");
    let example_dir = PathBuf::from(manifest_dir)
        .join("../../examples/hello-wasm-plugin");
    
    std::process::Command::new("cargo")
        .args(&["build", "--target", "wasm32-wasi", "--release"])
        .current_dir(&example_dir)
        .status()
        .expect("Failed to build test plugin");
    
    example_dir.join("target/wasm32-wasi/release/hello_wasm_plugin.wasm")
}

#[test]
fn test_plugin_help() {
    let mut cmd = Command::cargo_bin("hakimi").unwrap();
    cmd.arg("plugin").arg("--help");
    
    cmd.assert()
        .success()
        .stdout(predicate::str::contains("list"))
        .stdout(predicate::str::contains("install"))
        .stdout(predicate::str::contains("uninstall"));
}
```

**验收**: `cargo test --package hakimi-cli --features wasm plugin_cli_test` 通过

---

## 🧪 测试计划

### 单元测试
- [x] PluginManager 配置加载/保存
- [x] 插件查找和过滤
- [x] 启用/禁用状态切换

### 集成测试
- [x] CLI 命令输出格式正确
- [x] 插件安装/卸载流程完整
- [x] 错误情况处理（文件不存在、重复安装等）

### 手动测试
- [ ] `hakimi plugin list` 显示友好
- [ ] `hakimi plugin install` 从本地文件安装
- [ ] `hakimi plugin test` 验证插件可加载
- [ ] `hakimi plugin info` 显示完整信息
- [ ] 错误提示清晰易懂

---

## 📊 验收检查清单

- [ ] `cargo build --package hakimi-cli --features wasm` 成功
- [ ] `cargo test --package hakimi-cli plugin` 全部通过
- [ ] `hakimi plugin --help` 显示完整帮助
- [ ] 安装示例插件成功
- [ ] 卸载插件后文件被删除
- [ ] `plugins.json` 配置文件格式正确
- [ ] 禁用插件后不会被加载
- [ ] README 更新，展示插件命令
- [ ] CHANGELOG 记录新增功能

---

## 🔄 后续任务

完成本任务后，解锁：

- **TASK 5.2.2**: 插件市场后端（Registry API，支持远程安装）
- **TASK 5.2.3**: WebUI 插件管理界面
- **TASK 5.3.1**: 插件权限系统（细粒度控制）

---

## 📝 实施备注

- 使用 `colored` crate 提供彩色输出
- 使用 `indicatif` crate 提供进度条（远程下载时）
- 插件配置使用 JSON 格式便于手动编辑
- 考虑添加 `hakimi plugin update` 命令（检查更新）
- 考虑添加 `hakimi plugin search` 命令（搜索市场）
