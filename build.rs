use std::path::{Path, PathBuf};
use std::process::Command;
use std::{env, fs};

/// PDFium build matching the `pdfium_8125` feature of pdfium-render.
const PDFIUM_RELEASE: &str = "chromium/8125";

fn main() {
    // Texts are translated without context, so one entry in the .po file
    // covers a text wherever it appears; Rust reads the same file.
    let config = slint_build::CompilerConfiguration::new()
        .with_bundled_translations("translations")
        .with_default_translation_context(slint_build::DefaultTranslationContext::None);
    slint_build::compile_with_config("ui/app.slint", config)
        .expect("failed to compile ui/app.slint");
    println!("cargo:rerun-if-changed=translations");
    println!("cargo:rerun-if-changed=build.rs");

    println!("cargo:rerun-if-changed=assets/icon.ico");

    let os = env::var("CARGO_CFG_TARGET_OS").unwrap_or_default();
    if let Some(lib) = Pdfium::for_os(&os) {
        if let Err(err) = provide_pdfium(&lib) {
            println!("cargo:warning=could not provide {}: {err}", lib.file);
        }
    }
    if os == "windows" {
        let mut res = winresource::WindowsResource::new();
        res.set_icon("assets/icon.ico");
        if let Err(err) = res.compile() {
            println!("cargo:warning=could not embed the icon: {err}");
        }
    }
}

/// The prebuilt PDFium archive for a target OS and where its library sits.
struct Pdfium {
    archive: &'static str,
    dir: &'static str,
    file: &'static str,
}

impl Pdfium {
    fn for_os(os: &str) -> Option<Self> {
        match os {
            "windows" => Some(Self {
                archive: "pdfium-win-x64.tgz",
                dir: "bin",
                file: "pdfium.dll",
            }),
            "linux" => Some(Self {
                archive: "pdfium-linux-x64.tgz",
                dir: "lib",
                file: "libpdfium.so",
            }),
            _ => None,
        }
    }
}

/// Downloads the prebuilt PDFium library once (into `target/pdfium/`) and copies
/// it next to the built executable. Uses curl and tar (both ship with Windows).
fn provide_pdfium(lib: &Pdfium) -> Result<(), String> {
    let out_dir = PathBuf::from(env::var("OUT_DIR").map_err(|e| e.to_string())?);
    // OUT_DIR is target/<profile>/build/<pkg>[/<hash>]/out
    let profile_dir = out_dir
        .ancestors()
        .find(|dir| dir.file_name().is_some_and(|name| name == "build"))
        .and_then(Path::parent)
        .ok_or("unexpected OUT_DIR layout")?;
    let target_dir = profile_dir.parent().ok_or("unexpected OUT_DIR layout")?;

    let cache_dir = target_dir
        .join("pdfium")
        .join(PDFIUM_RELEASE.replace('/', "-"));
    let dll = cache_dir.join(lib.dir).join(lib.file);
    if !dll.exists() {
        download(&cache_dir, lib.archive)?;
    }
    let dest = profile_dir.join(lib.file);
    // Skip when already in place: the DLL is locked while Leme is running.
    let size = |p: &Path| fs::metadata(p).map(|m| m.len()).ok();
    if size(&dest).is_none() || size(&dest) != size(&dll) {
        fs::copy(&dll, dest).map_err(|e| e.to_string())?;
    }
    Ok(())
}

fn download(cache_dir: &Path, name: &str) -> Result<(), String> {
    fs::create_dir_all(cache_dir).map_err(|e| e.to_string())?;
    let archive = cache_dir.join(name);
    let url = format!(
        "https://github.com/bblanchon/pdfium-binaries/releases/download/{PDFIUM_RELEASE}/{name}"
    );
    run(Command::new("curl")
        .args(["-sSfL", "-o"])
        .arg(&archive)
        .arg(&url))?;
    run(Command::new("tar")
        .arg("-xzf")
        .arg(&archive)
        .arg("-C")
        .arg(cache_dir))?;
    fs::remove_file(&archive).map_err(|e| e.to_string())
}

fn run(cmd: &mut Command) -> Result<(), String> {
    let status = cmd.status().map_err(|e| e.to_string())?;
    if status.success() {
        Ok(())
    } else {
        Err(format!("{cmd:?} failed with {status}"))
    }
}
