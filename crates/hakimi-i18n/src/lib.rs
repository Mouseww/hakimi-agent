//! Lightweight internationalization (i18n) for Hakimi Agent.
//!
//! Uses locale YAML catalogs with dotted key paths and English fallback.

use std::collections::HashMap;
use std::ffi::OsStr;
use std::path::Path;
use tracing::{debug, warn};

/// Default locale used when no configured or environment language is supported.
pub const DEFAULT_LANGUAGE: &str = "en";

/// Hermes-compatible language catalogs Hakimi knows how to normalize.
pub const SUPPORTED_LANGUAGES: &[&str] = &[
    "en", "zh", "zh-hant", "ja", "de", "es", "fr", "tr", "uk", "af", "ko", "it", "ga", "pt", "ru",
    "hu",
];

/// Normalize a user-supplied language value to a supported locale code.
pub fn normalize_language(value: impl AsRef<str>) -> String {
    let key = value.as_ref().trim().to_ascii_lowercase();
    if key.is_empty() {
        return DEFAULT_LANGUAGE.to_string();
    }
    if SUPPORTED_LANGUAGES.contains(&key.as_str()) {
        return key;
    }

    let normalized = match key.as_str() {
        "english" | "en-us" | "en-gb" => "en",
        "chinese" | "mandarin" | "zh-cn" | "zh-hans" | "zh-sg" => "zh",
        "traditional-chinese" | "traditional_chinese" | "zh-tw" | "zh-hk" | "zh-mo" => "zh-hant",
        "japanese" | "jp" | "ja-jp" => "ja",
        "german" | "deutsch" | "de-de" | "de-at" | "de-ch" => "de",
        "spanish" | "espanol" | "español" | "es-es" | "es-mx" | "es-ar" => "es",
        "french" | "francais" | "français" | "france" | "fr-fr" | "fr-be" | "fr-ca" | "fr-ch" => {
            "fr"
        }
        "turkish" | "turkce" | "türkçe" | "tr-tr" => "tr",
        "ukrainian" | "ukrainisch" | "українська" | "uk-ua" | "ua" => "uk",
        "afrikaans" | "af-za" => "af",
        "korean" | "한국어" | "ko-kr" => "ko",
        "italian" | "italiano" | "it-it" | "it-ch" => "it",
        "irish" | "gaeilge" | "ga-ie" => "ga",
        "portuguese" | "portugues" | "português" | "pt-pt" | "pt-br" | "brazilian"
        | "brasileiro" => "pt",
        "russian" | "русский" | "ru-ru" => "ru",
        "hungarian" | "magyar" | "hu-hu" => "hu",
        _ => {
            let base = key.split('-').next().unwrap_or_default();
            if SUPPORTED_LANGUAGES.contains(&base) {
                base
            } else {
                DEFAULT_LANGUAGE
            }
        }
    };

    normalized.to_string()
}

/// Resolve language from environment-style key/value pairs.
///
/// `HAKIMI_LANGUAGE` takes precedence over `HERMES_LANGUAGE`. The function is
/// pure so tests do not need to mutate process-global environment variables.
pub fn language_from_env_pairs<'a>(
    pairs: impl IntoIterator<Item = (&'a str, &'a str)>,
) -> Option<String> {
    let mut hermes = None;
    for (key, value) in pairs {
        match key {
            "HAKIMI_LANGUAGE" if !value.trim().is_empty() => {
                return Some(normalize_language(value));
            }
            "HERMES_LANGUAGE" if !value.trim().is_empty() => {
                hermes = Some(normalize_language(value));
            }
            _ => {}
        }
    }
    hermes
}

/// Resolve the active language from process environment and configured display language.
pub fn resolve_language(configured_language: Option<&str>) -> String {
    let hakimi_language = std::env::var("HAKIMI_LANGUAGE").ok();
    let hermes_language = std::env::var("HERMES_LANGUAGE").ok();
    resolve_language_from_values(
        hakimi_language.as_deref(),
        hermes_language.as_deref(),
        configured_language,
    )
}

/// Resolve language from explicit values with env-style precedence.
pub fn resolve_language_from_values(
    hakimi_language: Option<&str>,
    hermes_language: Option<&str>,
    configured_language: Option<&str>,
) -> String {
    if let Some(value) = hakimi_language.filter(|value| !value.trim().is_empty()) {
        return normalize_language(value);
    }
    if let Some(value) = hermes_language.filter(|value| !value.trim().is_empty()) {
        return normalize_language(value);
    }
    configured_language
        .filter(|value| !value.trim().is_empty())
        .map(normalize_language)
        .unwrap_or_else(|| DEFAULT_LANGUAGE.to_string())
}

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
            locale: normalize_language(locale.into()),
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
            locale: normalize_language(locale),
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
        let locale = normalize_language(fallback_locale.into());
        Self {
            current_locale: locale.clone(),
            catalogs: HashMap::new(),
            fallback_locale: locale,
        }
    }

    /// Create an i18n instance and load all YAML catalogs from a directory.
    pub fn from_catalog_dir(
        current_locale: impl AsRef<str>,
        fallback_locale: impl AsRef<str>,
        dir: impl AsRef<Path>,
    ) -> anyhow::Result<Self> {
        let fallback_locale = normalize_language(fallback_locale);
        let mut i18n = Self::new(fallback_locale);
        i18n.set_locale(current_locale.as_ref());
        i18n.load_catalog_dir(dir)?;
        Ok(i18n)
    }

    /// Add a locale catalog.
    pub fn add_catalog(&mut self, catalog: LocaleCatalog) {
        self.catalogs.insert(catalog.locale.clone(), catalog);
    }

    /// Set the current locale.
    pub fn set_locale(&mut self, locale: &str) {
        self.current_locale = normalize_language(locale);
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

    /// Translate a key with named format arguments.
    pub fn tf_named(&self, key: &str, args: &[(&str, &str)]) -> String {
        let mut result = self.t(key);
        for (name, value) in args {
            result = result.replace(&format!("{{{name}}}"), value);
        }
        result
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

    /// Load all `.yaml` and `.yml` locale catalogs from a directory.
    pub fn load_catalog_dir(&mut self, dir: impl AsRef<Path>) -> anyhow::Result<usize> {
        let dir = dir.as_ref();
        let mut loaded = 0usize;
        for entry in std::fs::read_dir(dir)? {
            let entry = entry?;
            let path = entry.path();
            if !is_yaml_file(&path) {
                continue;
            }
            let Some(stem) = path.file_stem().and_then(OsStr::to_str) else {
                continue;
            };
            let locale = normalize_language(stem);
            let yaml = match std::fs::read_to_string(&path) {
                Ok(value) => value,
                Err(err) => {
                    warn!(path = %path.display(), error = %err, "Skipping unreadable locale catalog");
                    continue;
                }
            };
            match LocaleCatalog::from_yaml(&locale, &yaml) {
                Ok(catalog) => {
                    self.add_catalog(catalog);
                    loaded += 1;
                }
                Err(err) => {
                    warn!(path = %path.display(), error = %err, "Skipping invalid locale catalog");
                }
            }
        }
        Ok(loaded)
    }
}

fn is_yaml_file(path: &Path) -> bool {
    matches!(
        path.extension().and_then(OsStr::to_str),
        Some("yaml" | "yml")
    )
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
    use std::fs;
    use std::time::{SystemTime, UNIX_EPOCH};

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
    fn test_i18n_named_format() {
        let mut i18n = I18n::new("en");
        let mut en = LocaleCatalog::new("en");
        en.set("gateway.draining", "Draining {count} active agent(s)...");
        i18n.add_catalog(en);

        assert_eq!(
            i18n.tf_named("gateway.draining", &[("count", "3")]),
            "Draining 3 active agent(s)..."
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
        i18n.set_locale("fr-FR");
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

    #[test]
    fn test_normalize_language_aliases() {
        assert_eq!(normalize_language("zh-CN"), "zh");
        assert_eq!(normalize_language("traditional_chinese"), "zh-hant");
        assert_eq!(normalize_language("jp"), "ja");
        assert_eq!(normalize_language("pt-BR"), "pt");
        assert_eq!(normalize_language("unknown"), DEFAULT_LANGUAGE);
    }

    #[test]
    fn test_language_from_env_pairs_precedence() {
        let pairs = [("HERMES_LANGUAGE", "ja"), ("HAKIMI_LANGUAGE", "de-DE")];
        assert_eq!(language_from_env_pairs(pairs), Some("de".to_string()));
        assert_eq!(
            language_from_env_pairs([("HERMES_LANGUAGE", "zh-Hant")]),
            Some("zh-hant".to_string())
        );
        assert_eq!(language_from_env_pairs([("OTHER", "fr")]), None);
    }

    #[test]
    fn test_resolve_language_from_values() {
        assert_eq!(
            resolve_language_from_values(Some("fr-CA"), Some("ja"), Some("zh")),
            "fr"
        );
        assert_eq!(
            resolve_language_from_values(None, Some("ja-JP"), Some("zh")),
            "ja"
        );
        assert_eq!(
            resolve_language_from_values(None, None, Some("es-MX")),
            "es"
        );
        assert_eq!(
            resolve_language_from_values(None, None, Some("")),
            DEFAULT_LANGUAGE
        );
        assert_eq!(
            resolve_language_from_values(None, None, None),
            DEFAULT_LANGUAGE
        );
    }

    #[test]
    fn test_load_catalog_dir_with_fallback() {
        let dir = unique_temp_dir("hakimi-i18n-catalogs");
        fs::create_dir_all(&dir).expect("create temp locale dir");
        fs::write(
            dir.join("en.yaml"),
            "approval:\n  allow: \"Allow\"\n  deny: \"Deny\"\n",
        )
        .expect("write en catalog");
        fs::write(dir.join("zh-CN.yaml"), "approval:\n  allow: \"允许\"\n")
            .expect("write zh catalog");
        fs::write(dir.join("notes.txt"), "ignored").expect("write ignored file");

        let i18n = I18n::from_catalog_dir("zh-CN", "en", &dir).expect("load catalogs");
        assert_eq!(i18n.locale(), "zh");
        assert_eq!(i18n.t("approval.allow"), "允许");
        assert_eq!(i18n.t("approval.deny"), "Deny");
        assert_eq!(i18n.t("approval.missing"), "approval.missing");

        let _ = fs::remove_dir_all(dir);
    }

    fn unique_temp_dir(prefix: &str) -> std::path::PathBuf {
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("system clock")
            .as_nanos();
        std::env::temp_dir().join(format!("{prefix}-{nanos}"))
    }
}
