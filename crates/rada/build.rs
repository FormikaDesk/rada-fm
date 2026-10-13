//! On Windows, build rada's icon and version into `rada.exe` as resources, so that the
//! taskbar, Explorer and the shortcuts show the logo and the file's properties name the
//! program. Nothing happens for other targets.

use std::path::PathBuf;

fn main() {
    println!("cargo:rerun-if-changed=build.rs");
    println!("cargo:rerun-if-changed=assets/windows/rada.ico");
    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() != Ok("windows") {
        return;
    }
    // The resource compiler comes with the Windows build tools: a build for Windows made on
    // another system (to type-check it, say) goes without the resources.
    if !cfg!(windows) {
        println!(
            "cargo:warning=building for Windows from another system: no icon or version resource"
        );
        return;
    }
    let manifest = PathBuf::from(std::env::var("CARGO_MANIFEST_DIR").unwrap());
    let out = PathBuf::from(std::env::var("OUT_DIR").unwrap());
    let ico = manifest.join("assets/windows/rada.ico");
    let num = |k: &str| std::env::var(k).unwrap_or_else(|_| "0".into());
    let (major, minor, patch) = (
        num("CARGO_PKG_VERSION_MAJOR"),
        num("CARGO_PKG_VERSION_MINOR"),
        num("CARGO_PKG_VERSION_PATCH"),
    );
    let version = std::env::var("CARGO_PKG_VERSION").unwrap_or_default();
    let rc = format!(
        r#"1 ICON "{icon}"
1 VERSIONINFO
FILEVERSION {major},{minor},{patch},0
PRODUCTVERSION {major},{minor},{patch},0
BEGIN
  BLOCK "StringFileInfo"
  BEGIN
    BLOCK "040904b0"
    BEGIN
      VALUE "FileDescription", "rada: a safe harbor for your files"
      VALUE "ProductName", "rada"
      VALUE "InternalName", "rada"
      VALUE "OriginalFilename", "rada.exe"
      VALUE "FileVersion", "{version}"
      VALUE "ProductVersion", "{version}"
      VALUE "LegalCopyright", "MIT License"
    END
  END
  BLOCK "VarFileInfo"
  BEGIN
    VALUE "Translation", 0x409, 1200
  END
END
"#,
        icon = ico.display().to_string().replace('\\', "\\\\"),
    );
    let file = out.join("rada.rc");
    std::fs::write(&file, rc).expect("cannot write the resource script");
    embed_resource::compile(&file, embed_resource::NONE)
        .manifest_required()
        .expect("cannot build the icon and version resources into rada.exe");
}
