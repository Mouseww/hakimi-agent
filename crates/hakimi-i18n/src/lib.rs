//! Lightweight internationalization (i18n) for Hakimi Agent.
//!
//! Uses locale YAML catalogs with dotted key paths and English fallback.

use std::collections::HashMap;
use tracing::debug;

/// A locale catalog loaded from a YAML file.
#[derive(Debug, Clone)]
pub struct LocaleCatalog {
    /// Locale identifier (e.g. "en", "zh-CN", "ja").
    pub locale: String,
    /// Flat key-value map using dotted paths (e.g. "approval.allow").
    entries: HashMap<String, String>,
}

impl LocaleCatalog {
    /// Create a new empty catalog for a locale.
    pub fn new(locale: impl Into<String>) -> Self {
        Self {
            locale: locale.into(),
            entries: HashMap::new(),
        }
    }

    /// Load a catalog from a YAML string.
    pub fn from_yaml(locale: &str, yaml: &str) -> anyhow::Result<Self> {
        let raw: serde_yaml::Value = serde_yaml::from_str(yaml)?;
        let mut entries = HashMap::new();
        flatten_yaml(&raw, String::new(), &mut entries);
        debug!(locale, count = entries.len(), "Loaded locale catalog");
        Ok(Self {
            locale: locale.to_string(),
            entries,
        })
    }

    /// Look up a key, returning the translated string if found.
    pub fn get(&self, key: &str) -> Option<&str> {
        self.entries.get(key).map(|s| s.as_str())
    }

    /// Get a key with a fallback default.
    pub fn get_or<'a>(&'a self, key: &str, default: &'a str) -> &'a str {
        self.get(key).unwrap_or(default)
    }

    /// Insert a translation entry.
    pub fn set(&mut self, key: impl Into<String>, value: impl Into<String>) {
        self.entries.insert(key.into(), value.into());
    }

    /// Get the number of entries.
    pub fn len(&self) -> usize {
        self.entries.len()
    }

    /// Check if the catalog is empty.
    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }
}

/// The i18n system that manages multiple locale catalogs with fallback.
pub struct I18n {
    /// Current active locale.
    current_locale: String,
    /// Loaded catalogs by locale name.
    catalogs: HashMap<String, LocaleCatalog>,
    /// Default/fallback locale.
    fallback_locale: String,
}

impl I18n {
    /// Create a new i18n instance.
    pub fn new(fallback_locale: impl Into<String>) -> Self {
        let locale = fallback_locale.into();
        Self {
            current_locale: locale.clone(),
            catalogs: HashMap::new(),
            fallback_locale: locale,
        }
    }

    /// Add a locale catalog.
    pub fn add_catalog(&mut self, catalog: LocaleCatalog) {
        self.catalogs.insert(catalog.locale.clone(), catalog);
    }

    /// Set the current locale.
    pub fn set_locale(&mut self, locale: &str) {
        self.current_locale = locale.to_string();
    }

    /// Get the current locale.
    pub fn locale(&self) -> &str {
        &self.current_locale
    }

    /// Translate a key using the current locale, falling back to the default locale.
    pub fn t(&self, key: &str) -> String {
        // Try current locale first.
        if let Some(catalog) = self.catalogs.get(&self.current_locale)
            && let Some(value) = catalog.get(key)
        {
            return value.to_string();
        }

        // Fall back to fallback locale.
        if self.current_locale != self.fallback_locale
            && let Some(catalog) = self.catalogs.get(&self.fallback_locale)
            && let Some(value) = catalog.get(key)
        {
            return value.to_string();
        }

        // Return the key itself as a last resort.
        debug!(key, "Translation not found, returning key");
        key.to_string()
    }

    /// Translate a key with format arguments.
    ///
    /// The translated string can contain `{0}`, `{1}`, etc. placeholders
    /// that are replaced with the corresponding arguments.
    pub fn tf(&self, key: &str, args: &[&str]) -> String {
        let mut result = self.t(key);
        for (i, arg) in args.iter().enumerate() {
            result = result.replace(&format!("{{{}}}", i), arg);
        }
        result
    }
}

/// Flatten a nested YAML value into dotted key-value pairs.
fn flatten_yaml(value: &serde_yaml::Value, prefix: String, entries: &mut HashMap<String, String>) {
    match value {
        serde_yaml::Value::Mapping(map) => {
            for (key, val) in map {
                if let Some(key_str) = key.as_str() {
                    let new_key = if prefix.is_empty() {
                        key_str.to_string()
                    } else {
                        format!("{}.{}", prefix, key_str)
                    };
                    flatten_yaml(val, new_key, entries);
                }
            }
        }
        serde_yaml::Value::String(s) => {
            entries.insert(prefix, s.clone());
        }
        serde_yaml::Value::Number(n) => {
            entries.insert(prefix, n.to_string());
        }
        serde_yaml::Value::Bool(b) => {
            entries.insert(prefix, b.to_string());
        }
        _ => {}
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_catalog_from_yaml() {
        let yaml = r#"
approval:
  allow: "Allow"
  deny: "Deny"
restart:
  message: "Agent restarted"
"#;
        let catalog = LocaleCatalog::from_yaml("en", yaml).unwrap();
        assert_eq!(catalog.get("approval.allow"), Some("Allow"));
        assert_eq!(catalog.get("approval.deny"), Some("Deny"));
        assert_eq!(catalog.get("restart.message"), Some("Agent restarted"));
        assert_eq!(catalog.get("nonexistent"), None);
    }

    #[test]
    fn test_catalog_get_or() {
        let mut catalog = LocaleCatalog::new("en");
        catalog.set("greeting", "Hello");
        assert_eq!(catalog.get_or("greeting", "Hi"), "Hello");
        assert_eq!(catalog.get_or("missing", "default"), "default");
    }

    #[test]
    fn test_catalog_len() {
        let mut catalog = LocaleCatalog::new("en");
        assert!(catalog.is_empty());
        catalog.set("a", "1");
        catalog.set("b", "2");
        assert_eq!(catalog.len(), 2);
    }

    #[test]
    fn test_i18n_translate() {
        let mut i18n = I18n::new("en");
        let mut en = LocaleCatalog::new("en");
        en.set("greeting", "Hello");
        en.set("farewell", "Goodbye");
        i18n.add_catalog(en);

        let mut zh = LocaleCatalog::new("zh");
        zh.set("greeting", "你好");
        i18n.add_catalog(zh);

        i18n.set_locale("zh");
        assert_eq!(i18n.t("greeting"), "你好");
        // Fallback to English for missing key.
        assert_eq!(i18n.t("farewell"), "Goodbye");
        // Key not in any locale returns key itself.
        assert_eq!(i18n.t("unknown"), "unknown");
    }

    #[test]
    fn test_i18n_format() {
        let mut i18n = I18n::new("en");
        let mut en = LocaleCatalog::new("en");
        en.set("welcome", "Hello, {0}! You have {1} messages.");
        i18n.add_catalog(en);

        assert_eq!(
            i18n.tf("welcome", &["Alice", "5"]),
            "Hello, Alice! You have 5 messages."
        );
    }

    #[test]
    fn test_flatten_yaml_nested() {
        let yaml = r#"
level1:
  level2:
    key: "value"
"#;
        let catalog = LocaleCatalog::from_yaml("en", yaml).unwrap();
        assert_eq!(catalog.get("level1.level2.key"), Some("value"));
    }

    #[test]
    fn test_catalog_set_and_get() {
        let mut catalog = LocaleCatalog::new("test");
        catalog.set("key1", "value1");
        assert_eq!(catalog.get("key1"), Some("value1"));
    }

    #[test]
    fn test_i18n_same_locale_fallback() {
        let mut i18n = I18n::new("en");
        let mut en = LocaleCatalog::new("en");
        en.set("only_en", "English only");
        i18n.add_catalog(en);
        i18n.set_locale("en");
        assert_eq!(i18n.t("only_en"), "English only");
    }

    #[test]
    fn test_locale_accessor() {
        let mut i18n = I18n::new("en");
        assert_eq!(i18n.locale(), "en");
        i18n.set_locale("fr");
        assert_eq!(i18n.locale(), "fr");
    }

    #[test]
    fn test_catalog_from_yaml_numbers() {
        let yaml = r#"
count: 42
flag: true
"#;
        let catalog = LocaleCatalog::from_yaml("en", yaml).unwrap();
        assert_eq!(catalog.get("count"), Some("42"));
        assert_eq!(catalog.get("flag"), Some("true"));
    }
}
