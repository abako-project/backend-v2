//! Independent browser application; the native target only validates shared logic.
#[cfg(target_arch = "wasm32")]
mod browser;

fn main() {
    #[cfg(target_arch = "wasm32")]
    leptos::mount::mount_to_body(browser::App);
}
