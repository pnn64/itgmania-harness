use std::path::{Path, PathBuf};

pub fn theme_root() -> PathBuf {
    let theme = Path::new(env!("CARGO_MANIFEST_DIR")).join("vendor/simply-love");
    assert!(
        theme.join("Scripts/SL-ChartParser.lua").is_file(),
        "initialize Simply Love with `git submodule update --init`"
    );
    theme
}
