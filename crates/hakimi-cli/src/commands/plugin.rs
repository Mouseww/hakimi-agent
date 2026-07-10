use anyhow::{Context, Result};
use clap::{Parser, Subcommand};
use hakimi_plugin::marketplace::PluginMarketplace;
use std::path::PathBuf;
use tabled::{
    builder::Builder,
    settings::{object::Columns, Modify, Width, Style},
};

#[derive(Parser)]
#[command(name = "plugin")]
#[command(about = "Manage Hakimi plugins")]
pub struct PluginCommand {
    #[command(subcommand)]
    pub action: PluginAction,
}

#[derive(Subcommand)]
pub enum PluginAction {
    /// List installed or available plugins
    List {
        /// Show available plugins from marketplace
        #[arg(long)]
        available: bool,
    },

    /// Search for plugins
    Search {
        /// Search query (name, description)
        query: String,
    },

    /// Install a plugin
    Install {
        /// Plugin name
        name: String,

        /// Specific version (default: latest)
        #[arg(long)]
        version: Option<String>,
    },

    /// Uninstall a plugin
    Uninstall {
        /// Plugin name
        name: String,
    },

    /// Check for plugin updates
    Update {
        /// Plugin name (default: all)
        name: Option<String>,
    },

    /// Show plugin information
    Info {
        /// Plugin name
        name: String,
    },
}

impl PluginCommand {
    pub async fn execute(self) -> Result<()> {
        let marketplace = create_marketplace()?;

        match self.action {
            PluginAction::List { available } => {
                if available {
                    list_available(&marketplace).await?;
                } else {
                    list_installed(&marketplace)?;
                }
            }
            PluginAction::Search { query } => {
                search_plugins(&marketplace, &query).await?;
            }
            PluginAction::Install { name, version } => {
                install_plugin(&marketplace, &name, version.as_deref()).await?;
            }
            PluginAction::Uninstall { name } => {
                uninstall_plugin(&marketplace, &name)?;
            }
            PluginAction::Update { name } => {
                check_updates(&marketplace, name.as_deref()).await?;
            }
            PluginAction::Info { name } => {
                show_plugin_info(&marketplace, &name).await?;
            }
        }

        Ok(())
    }
}

fn create_marketplace() -> Result<PluginMarketplace> {
    let home = dirs::home_dir()
        .context("Failed to find home directory")?;
    let hakimi_dir = home.join(".hakimi");
    let cache_dir = hakimi_dir.join("cache");
    let plugins_dir = hakimi_dir.join("plugins");

    // 默认注册表 URL (可以从配置读取)
    let registry_url =
        std::env::var("HAKIMI_PLUGIN_REGISTRY")
            .unwrap_or_else(|_| {
                "https://raw.githubusercontent.com/hakimi-team/hakimi-agent/main/registry/plugins_registry.yaml"
                    .to_string()
            });

    PluginMarketplace::new(registry_url, cache_dir, plugins_dir)
}

async fn list_available(marketplace: &PluginMarketplace) -> Result<()> {
    println!("📦 Fetching available plugins...\n");

    let registry = marketplace
        .fetch_registry()
        .await
        .context("Failed to fetch plugin registry")?;

    if registry.plugins.is_empty() {
        println!("No plugins available.");
        return Ok(());
    }

    let mut builder = Builder::default();
    builder.push_record(["Name", "Version", "Author", "Description"]);

    for plugin in &registry.plugins {
        builder.push_record([
            &plugin.name,
            &plugin.version,
            &plugin.author,
            truncate(&plugin.description, 50),
        ]);
    }

    let mut table = builder.build();
    table.with(Style::rounded());
    table.with(Modify::new(Columns::single(3)).with(Width::truncate(50).suffix("...")));

    println!("{}", table);
    println!("\n{} plugins available", registry.plugins.len());

    Ok(())
}

fn list_installed(marketplace: &PluginMarketplace) -> Result<()> {
    let installed = marketplace.list_installed()?;

    if installed.is_empty() {
        println!("No plugins installed.");
        println!("\nTry: hakimi plugin list --available");
        return Ok(());
    }

    let mut builder = Builder::default();
    builder.push_record(["Name", "Version", "Enabled", "Installed At"]);

    for plugin in &installed {
        let enabled_icon = if plugin.enabled { "✓" } else { "✗" };
        builder.push_record([
            &plugin.name,
            &plugin.version,
            enabled_icon,
            &plugin.installed_at,
        ]);
    }

    let mut table = builder.build();
    table.with(Style::rounded());

    println!("📦 Installed Plugins\n");
    println!("{}", table);
    println!("\n{} plugins installed", installed.len());

    Ok(())
}

async fn search_plugins(marketplace: &PluginMarketplace, query: &str) -> Result<()> {
    println!("🔍 Searching for '{}'...\n", query);

    let results = marketplace
        .search(query)
        .await
        .context("Failed to search plugins")?;

    if results.is_empty() {
        println!("No plugins found matching '{}'", query);
        return Ok(());
    }

    let mut builder = Builder::default();
    builder.push_record(["Name", "Version", "Author", "Description"]);

    for plugin in &results {
        builder.push_record([
            &plugin.name,
            &plugin.version,
            &plugin.author,
            truncate(&plugin.description, 50),
        ]);
    }

    let mut table = builder.build();
    table.with(Style::rounded());
    table.with(Modify::new(Columns::single(3)).with(Width::truncate(50).suffix("...")));

    println!("{}", table);
    println!("\n{} plugins found", results.len());

    Ok(())
}

async fn install_plugin(
    marketplace: &PluginMarketplace,
    name: &str,
    version: Option<&str>,
) -> Result<()> {
    println!("📦 Installing plugin '{}'...", name);

    if let Some(v) = version {
        println!("   Version: {}", v);
    }

    let installed = marketplace
        .install_plugin(name, version)
        .await
        .context("Failed to install plugin")?;

    println!("\n✓ Successfully installed:");
    println!("  Name:    {}", installed.name);
    println!("  Version: {}", installed.version);
    println!("  Path:    {}", installed.path);

    Ok(())
}

fn uninstall_plugin(marketplace: &PluginMarketplace, name: &str) -> Result<()> {
    println!("🗑️  Uninstalling plugin '{}'...", name);

    marketplace
        .uninstall_plugin(name)
        .context("Failed to uninstall plugin")?;

    println!("✓ Successfully uninstalled '{}'", name);

    Ok(())
}

async fn check_updates(
    marketplace: &PluginMarketplace,
    name: Option<&str>,
) -> Result<()> {
    println!("🔄 Checking for updates...\n");

    let updates = marketplace
        .check_updates()
        .await
        .context("Failed to check updates")?;

    let filtered_updates: Vec<_> = if let Some(filter_name) = name {
        updates
            .into_iter()
            .filter(|u| u.name == filter_name)
            .collect()
    } else {
        updates
    };

    if filtered_updates.is_empty() {
        println!("✓ All plugins are up to date!");
        return Ok(());
    }

    let mut builder = Builder::default();
    builder.push_record(["Plugin", "Current", "Latest"]);

    for update in &filtered_updates {
        builder.push_record([
            &update.name,
            &update.current_version,
            &update.latest_version,
        ]);
    }

    let mut table = builder.build();
    table.with(Style::rounded());

    println!("{}", table);
    println!("\n{} updates available", filtered_updates.len());
    println!("\nTo update, run: hakimi plugin install <name>");

    Ok(())
}

async fn show_plugin_info(marketplace: &PluginMarketplace, name: &str) -> Result<()> {
    let registry = marketplace
        .fetch_registry()
        .await
        .context("Failed to fetch registry")?;

    let plugin = registry
        .plugins
        .iter()
        .find(|p| p.name == name)
        .context(format!("Plugin '{}' not found", name))?;

    println!("📦 Plugin Information\n");
    println!("Name:        {}", plugin.display_name);
    println!("ID:          {}", plugin.name);
    println!("Version:     {}", plugin.version);
    println!("Author:      {}", plugin.author);
    println!("Description: {}", plugin.description);
    println!("Repository:  {}", plugin.repository);
    println!("\nPlatforms:");
    for (platform, binary) in &plugin.platforms {
        println!("  - {}: {}", platform, binary);
    }

    // Check if installed
    let installed = marketplace.list_installed()?;
    if let Some(local) = installed.iter().find(|p| p.name == name) {
        println!("\nInstalled:");
        println!("  Version:      {}", local.version);
        println!("  Installed at: {}", local.installed_at);
        println!("  Path:         {}", local.path);
    } else {
        println!("\nNot installed.");
        println!("To install: hakimi plugin install {}", name);
    }

    Ok(())
}

fn truncate(s: &str, max_len: usize) -> &str {
    if s.len() <= max_len {
        s
    } else {
        &s[..max_len]
    }
}
