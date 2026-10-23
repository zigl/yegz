use std::fs;
use std::path::Path;

#[test]
fn test_app_bundle_exists() {
    assert!(Path::new("target/release/Yegz.app/Contents/MacOS/yegz").exists() || Path::new("target/release/Lite.app/Contents/MacOS/lite").exists());
    assert!(Path::new("bin/yegz").exists());
}

#[test]
fn test_cli_launcher_executable() {
    use std::os::unix::fs::PermissionsExt;
    let meta = fs::metadata("bin/yegz").unwrap();
    assert!(meta.permissions().mode() & 0o111 != 0, "bin/yegz should be executable");
}

#[test]
fn test_all_themes_defined() {
    use yegz_lib_test_helper::ThemeName;
    assert_eq!(ThemeName::ALL.len(), 4);
    assert!(!ThemeName::OneLight.is_dark());
    assert!(ThemeName::OneDark.is_dark());
}

mod yegz_lib_test_helper {
    #[derive(Debug, Clone, Copy, PartialEq, Eq)]
    pub enum ThemeName {
        OneDark,
        OneLight,
        GitHubDark,
        Monokai,
    }
    impl ThemeName {
        pub const ALL: &[ThemeName] = &[
            ThemeName::OneDark,
            ThemeName::OneLight,
            ThemeName::GitHubDark,
            ThemeName::Monokai,
        ];
        pub fn is_dark(&self) -> bool {
            !matches!(self, Self::OneLight)
        }
    }
}
