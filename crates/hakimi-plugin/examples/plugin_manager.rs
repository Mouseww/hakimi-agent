use anyhow::Result;
use hakimi_plugin::marketplace::PluginMarketplace;

#[tokio::main]
async fn main() -> Result<()> {

    let args: Vec<String> = std::env::args().collect();
    
    if args.len() < 2 {
        print_usage();
        return Ok(());
    }

    let marketplace = create_marketplace()?;

    match args[1].as_str() {
        "list" => {
            list_installed(&marketplace)?;
        }
        "available" => {
            list_available(&marketplace).await?;
        }
        "search" => {
            if args.len() < 3 {
                eprintln!("Usage: hakimi-plugin search <query>");
                return Ok(());
            }
            search(&marketplace, &args[2]).await?;
        }
        "info" => {
            if args.len() < 3 {
                eprintln!("Usage: hakimi-plugin info <name>");
                return Ok(());
            }
            info(&marketplace, &args[2]).await?;
        }
        "install" => {
            if args.len() < 3 {
                eprintln!("Usage: hakimi-plugin install <name> [version]");
                return Ok(());
            }
            let version = args.get(3).map(|s| s.as_str());
            install(&marketplace, &args[2], version).await?;
        }
        "uninstall" => {
            if args.len() < 3 {
                eprintln!("Usage: hakimi-plugin uninstall <name>");
                return Ok(());
            }
            uninstall(&marketplace, &args[2])?;
        }
        "updates" => {
            check_updates(&marketplace).await?;
        }
        _ => {
            eprintln!("Unknown command: {}", args[1]);
            print_usage();
        }
    }

    Ok(())
}

fn print_usage() {
    println!("Hakimi Plugin Manager");
    println!("\nUsage: hakimi-plugin <command> [args]");
    println!("\nCommands:");
    println!("  list              List installed plugins");
    println!("  available         List available plugins from registry");
    println!("  search <query>    Search for plugins");
    println!("  info <name>       Show plugin information");
    println!("  install <name>    Install a plugin");
    println!("  uninstall <name>  Uninstall a plugin");
    println!("  updates           Check for plugin updates");
}

fn create_marketplace() -> Result<PluginMarketplace> {
    let home = dirs::home_dir().expect("Failed to find home directory");
    let hakimi_dir = home.join(".hakimi");
    let cache_dir = hakimi_dir.join("cache");
    let plugins_dir = hakimi_dir.join("plugins");

    // 使用本地注册表进行测试
    let registry_url = std::env::var("HAKIMI_PLUGIN_REGISTRY")
        .unwrap_or_else(|_| {
            "file://".to_string() + 
            &std::env::current_dir()
                .unwrap()
                .join("registry/plugins_registry.yaml")
                .to_string_lossy()
        });

    PluginMarketplace::new(registry_url, cache_dir, plugins_dir)
}

fn list_installed(marketplace: &PluginMarketplace) -> Result<()> {
    let installed = marketplace.list_installed()?;

    if installed.is_empty() {
        println!("No plugins installed.");
        return Ok(());
    }

    println!("📦 Installed Plugins:\n");
    for plugin in installed {
        println!("  • {} v{}", plugin.name, plugin.version);
        println!("    Enabled: {}", if plugin.enabled { "✓" } else { "✗" });
        println!("    Path: {}", plugin.path);
        println!();
    }

    Ok(())
}

async fn list_available(marketplace: &PluginMarketplace) -> Result<()> {
    println!("📦 Fetching available plugins...\n");

    let registry = marketplace.fetch_registry().await?;

    if registry.plugins.is_empty() {
        println!("No plugins available.");
        return Ok(());
    }

    println!("Available Plugins:\n");
    for plugin in &registry.plugins {
        println!("  • {} v{}", plugin.name, plugin.version);
        println!("    {}", plugin.description);
        println!("    Author: {}", plugin.author);
        println!();
    }

    println!("Total: {} plugins", registry.plugins.len());

    Ok(())
}

async fn search(marketplace: &PluginMarketplace, query: &str) -> Result<()> {
    println!("🔍 Searching for '{}'...\n", query);

    let results = marketplace.search(query).await?;

    if results.is_empty() {
        println!("No plugins found.");
        return Ok(());
    }

    for plugin in &results {
        println!("  • {} v{}", plugin.name, plugin.version);
        println!("    {}", plugin.description);
        println!();
    }

    println!("Found: {} plugins", results.len());

    Ok(())
}

async fn info(marketplace: &PluginMarketplace, name: &str) -> Result<()> {
    let registry = marketplace.fetch_registry().await?;

    let plugin = registry
        .plugins
        .iter()
        .find(|p| p.name == name)
        .ok_or_else(|| anyhow::anyhow!("Plugin '{}' not found", name))?;

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
        println!("\n✓ Installed:");
        println!("  Version:      {}", local.version);
        println!("  Installed at: {}", local.installed_at);
        println!("  Path:         {}", local.path);
    } else {
        println!("\n✗ Not installed");
    }

    Ok(())
}

async fn install(
    marketplace: &PluginMarketplace,
    name: &str,
    version: Option<&str>,
) -> Result<()> {
    println!("📦 Installing plugin '{}'...", name);

    if let Some(v) = version {
        println!("   Version: {}", v);
    }

    let installed = marketplace.install_plugin(name, version).await?;

    println!("\n✓ Successfully installed:");
    println!("  Name:    {}", installed.name);
    println!("  Version: {}", installed.version);
    println!("  Path:    {}", installed.path);

    Ok(())
}

fn uninstall(marketplace: &PluginMarketplace, name: &str) -> Result<()> {
    println!("🗑️  Uninstalling plugin '{}'...", name);

    marketplace.uninstall_plugin(name)?;

    println!("✓ Successfully uninstalled '{}'", name);

    Ok(())
}

async fn check_updates(marketplace: &PluginMarketplace) -> Result<()> {
    println!("🔄 Checking for updates...\n");

    let updates = marketplace.check_updates().await?;

    if updates.is_empty() {
        println!("✓ All plugins are up to date!");
        return Ok(());
    }

    for update in &updates {
        println!(
            "  • {} {} → {}",
            update.name, update.current_version, update.latest_version
        );
    }

    println!("\n{} updates available", updates.len());

    Ok(())
}
