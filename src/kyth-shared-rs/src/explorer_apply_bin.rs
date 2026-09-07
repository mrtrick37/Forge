//! Native replacement for the Python `kyth-apply-explorer` launcher.
//!
//! Loads `explorer.toml` (see `system::explorer_preset::load_explorer`) and
//! applies it via `system::explorer_preset::apply_explorer`, reproducing the
//! Python launcher's output line and behavior — including the dead
//! `kwriteconfig5` path on the shipped Kinoite 44 image; see that function's
//! doc comment.

use kyth_shared::system::explorer_preset::{apply_explorer, explorer_path, load_explorer};

fn main() {
    let config = load_explorer(explorer_path(None::<&std::path::Path>));
    let applied = apply_explorer(&config);
    println!("kyth-apply-explorer: {}", applied.len());
}
