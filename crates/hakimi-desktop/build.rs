// Only invoke tauri-build when the `gui` feature is enabled.
fn main() {
    #[cfg(feature = "gui")]
    tauri_build::build();
}
