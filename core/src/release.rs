//! Updates, and the signature that makes them safe to install.
//!
//! A confirmation dialog is not a security control: it asks you to approve
//! something you cannot inspect, and you would click yes to somebody else's
//! build exactly as readily as to your own. What makes an update safe is that
//! the release is signed by a key that lives nowhere near where it is
//! published — so taking over the account it is published from is not enough.
//!
//! The public key is compiled in below. The private key is generated once, by a
//! person, and kept in a password manager; nothing here ever reads it except
//! `collab release sign`, and then only from an argument.
use crate::config;
use base64::engine::general_purpose::STANDARD as B64;
use base64::Engine as _;
use ed25519_dalek::{Signature, Signer, SigningKey, Verifier, VerifyingKey};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

/// Set by `collab release keygen`, then pasted here and committed. A public key
/// in a public repository is exactly where a public key belongs.
pub const PUBLIC_KEY: &str = "";

pub const VERSION: &str = env!("CARGO_PKG_VERSION");
pub const MANIFEST: &str = "collab-release.json";

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct Artifact {
    pub sha256: String,
    pub size: u64,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct Manifest {
    pub version: String,
    #[serde(default)]
    pub notes: String,
    /// Path within the release, e.g. "macos-arm64/collab".
    pub files: BTreeMap<String, Artifact>,
}

fn verifying_key() -> Option<VerifyingKey> {
    let raw = B64.decode(PUBLIC_KEY.trim()).ok()?;
    let bytes: [u8; 32] = raw.try_into().ok()?;
    VerifyingKey::from_bytes(&bytes).ok()
}

/// Where releases are fetched from. Nothing is downloaded until this is set,
/// so a machine with no update source simply has no update button that works.
pub fn source() -> Option<String> {
    let u = config::env("COLLAB_UPDATE_URL", "");
    (!u.is_empty()).then(|| u.trim_end_matches('/').to_string())
}

// ───────────────────────────── making a release ─────────────────────────────

pub fn keygen() {
    use rand::rngs::OsRng;
    let signing = SigningKey::generate(&mut OsRng);
    println!("private key — put this in your password manager and nowhere else:\n");
    println!("    {}\n", B64.encode(signing.to_bytes()));
    println!("public key — paste into PUBLIC_KEY in core/src/release.rs and commit it:\n");
    println!("    {}\n", B64.encode(signing.verifying_key().to_bytes()));
    println!("if you lose the private key, updates stop until you hand-deliver a build");
    println!("carrying a new public key. If it leaks, do the same, urgently.");
}

/// "-" means read it from stdin. A key passed as an argument is visible in the
/// process list for as long as the command runs, and in shell history for ever.
fn read_private(key_b64: &str) -> Result<SigningKey, String> {
    let key_b64 = if key_b64.trim() == "-" {
        let mut s = String::new();
        std::io::stdin().read_line(&mut s).map_err(|e| e.to_string())?;
        s
    } else {
        key_b64.to_string()
    };
    let key_b64 = key_b64.as_str();
    let raw = B64
        .decode(key_b64.trim())
        .map_err(|_| "the private key is not valid base64".to_string())?;
    let bytes: [u8; 32] = raw
        .try_into()
        .map_err(|_| "the private key is the wrong length".to_string())?;
    Ok(SigningKey::from_bytes(&bytes))
}

/// Builds a manifest describing everything in `dir`, and signs it.
pub fn sign_release(dir: &Path, version: &str, notes: &str, key_b64: &str) -> Result<(), String> {
    let signing = read_private(key_b64)?;
    let mut files = BTreeMap::new();
    collect(dir, dir, &mut files)?;
    if files.is_empty() {
        return Err(format!("nothing to sign in {}", dir.display()));
    }
    let manifest = Manifest {
        version: version.to_string(),
        notes: notes.to_string(),
        files,
    };
    // The signature covers the exact bytes written, not a re-serialisation of
    // them: anything else leaves room for the two to differ.
    let body = serde_json::to_vec_pretty(&manifest).map_err(|e| e.to_string())?;
    let sig = signing.sign(&body);
    std::fs::write(dir.join(MANIFEST), &body).map_err(|e| e.to_string())?;
    std::fs::write(dir.join(format!("{MANIFEST}.sig")), B64.encode(sig.to_bytes()))
        .map_err(|e| e.to_string())?;
    println!("signed {} file(s) as version {version}", manifest.files.len());
    for name in manifest.files.keys() {
        println!("  {name}");
    }
    Ok(())
}

fn collect(root: &Path, dir: &Path, out: &mut BTreeMap<String, Artifact>) -> Result<(), String> {
    for entry in std::fs::read_dir(dir).map_err(|e| e.to_string())?.flatten() {
        let path = entry.path();
        let name = path.file_name().unwrap_or_default().to_string_lossy().to_string();
        if name.starts_with('.') || name.starts_with(MANIFEST) {
            continue;
        }
        if path.is_dir() {
            collect(root, &path, out)?;
        } else {
            let data = std::fs::read(&path).map_err(|e| e.to_string())?;
            let rel = path
                .strip_prefix(root)
                .map_err(|e| e.to_string())?
                .to_string_lossy()
                .to_string();
            out.insert(
                rel,
                Artifact { sha256: crate::files::hash_bytes(&data), size: data.len() as u64 },
            );
        }
    }
    Ok(())
}

// ───────────────────────────── taking one ─────────────────────────────

/// curl rather than a TLS stack of our own: it is present on both machines,
/// and a whole HTTPS client linked in to fetch two files a month is a poor
/// trade against a binary that currently fits in half a megabyte.
fn fetch(url: &str) -> Result<Vec<u8>, String> {
    let out = std::process::Command::new("curl")
        .args(["-fsSL", "--max-time", "120", url])
        .output()
        .map_err(|e| format!("cannot run curl — {e}"))?;
    if !out.status.success() {
        return Err(format!("could not fetch {url}"));
    }
    Ok(out.stdout)
}

pub struct Available {
    pub manifest: Manifest,
    pub base: String,
}

/// Fetches the manifest and refuses it unless it is signed by the key built
/// into this binary. Everything downstream trusts this having happened.
pub fn check() -> Result<Available, String> {
    let base = source().ok_or_else(|| {
        "no update source set — put `update_url = …` in ~/.collab-config".to_string()
    })?;
    let key = verifying_key()
        .ok_or_else(|| "this build carries no release key, so it cannot verify an update".to_string())?;

    let body = fetch(&format!("{base}/{MANIFEST}"))?;
    let sig_raw = fetch(&format!("{base}/{MANIFEST}.sig"))?;
    let sig_bytes = B64
        .decode(String::from_utf8_lossy(&sig_raw).trim())
        .map_err(|_| "the signature is not valid base64".to_string())?;
    let sig_bytes: [u8; 64] = sig_bytes
        .try_into()
        .map_err(|_| "the signature is the wrong length".to_string())?;
    let sig = Signature::from_bytes(&sig_bytes);

    key.verify(&body, &sig)
        .map_err(|_| "the release is not signed by the key this build trusts — refusing it".to_string())?;

    let manifest: Manifest =
        serde_json::from_slice(&body).map_err(|_| "the release manifest is malformed".to_string())?;
    Ok(Available { manifest, base })
}

pub fn platform_files() -> Vec<&'static str> {
    if cfg!(target_os = "macos") {
        vec!["macos-arm64/collab", "macos-arm64/Collab.app.tar.gz"]
    } else {
        vec!["windows-x64/collab.exe", "windows-x64/collab-notify.exe", "windows-x64/collab.png"]
    }
}

fn staging(version: &str) -> PathBuf {
    config::home(".collab-updates").join(crate::files::safe_component(version))
}

/// Downloads what this platform needs, checking every file against the hash in
/// the manifest that was signed. Nothing is installed from here.
pub fn download(av: &Available) -> Result<PathBuf, String> {
    let dir = staging(&av.manifest.version);
    std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
    for name in platform_files() {
        let Some(want) = av.manifest.files.get(name) else {
            continue; // a release need not carry every platform
        };
        let data = fetch(&format!("{}/{name}", av.base))?;
        if data.len() as u64 != want.size || crate::files::hash_bytes(&data) != want.sha256 {
            return Err(format!("{name} does not match the signed manifest — nothing installed"));
        }
        let out = dir.join(crate::files::safe_component(
            Path::new(name).file_name().unwrap_or_default().to_string_lossy().as_ref(),
        ));
        std::fs::write(&out, &data).map_err(|e| e.to_string())?;
    }
    Ok(dir)
}

/// Puts a staged, verified update in place.
///
/// The delete-then-write is not fussiness. Writing over a signed binary in
/// place leaves macOS holding a stale code signature and the kernel kills the
/// result on sight, with no error message at all — a failure mode this project
/// has already walked into once by hand.
pub fn install(dir: &Path) -> Result<Vec<String>, String> {
    let mut done = Vec::new();

    let bin = dir.join(if cfg!(target_os = "windows") { "collab.exe" } else { "collab" });
    if bin.exists() {
        let target = std::env::current_exe().map_err(|e| e.to_string())?;
        let target = std::fs::canonicalize(&target).unwrap_or(target);
        if cfg!(target_os = "windows") {
            // A running executable cannot be overwritten on Windows, but it can
            // be renamed out of the way, and the old one swept up next time.
            let _ = std::fs::rename(&target, target.with_extension("exe.old"));
        } else {
            let _ = std::fs::remove_file(&target);
        }
        std::fs::copy(&bin, &target).map_err(|e| format!("cannot replace {} — {e}", target.display()))?;
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let _ = std::fs::set_permissions(&target, std::fs::Permissions::from_mode(0o755));
        }
        done.push(target.display().to_string());
    }

    let app_tar = dir.join("Collab.app.tar.gz");
    if app_tar.exists() {
        let apps = config::home("Applications");
        let _ = std::fs::create_dir_all(&apps);
        let _ = std::fs::remove_dir_all(apps.join("Collab.app"));
        let ok = std::process::Command::new("tar")
            .args(["-xzf"])
            .arg(&app_tar)
            .arg("-C")
            .arg(&apps)
            .status()
            .map(|s| s.success())
            .unwrap_or(false);
        if !ok {
            return Err("could not unpack Collab.app".into());
        }
        done.push(apps.join("Collab.app").display().to_string());
    }

    for extra in ["collab-notify.exe", "collab.png"] {
        let from = dir.join(extra);
        if from.exists() {
            if let Ok(exe) = std::env::current_exe() {
                if let Some(parent) = exe.parent() {
                    let to = parent.join(extra);
                    let _ = std::fs::remove_file(&to);
                    if std::fs::copy(&from, &to).is_ok() {
                        done.push(to.display().to_string());
                    }
                }
            }
        }
    }

    if done.is_empty() {
        return Err("the release carried nothing for this platform".into());
    }
    Ok(done)
}

/// `collab update` — reports; `-yes` installs. Requiring the word means a
/// script cannot update a machine by accident, and the app asks a person first.
pub fn update_cmd(install_it: bool, as_json: bool) {
    let av = match check() {
        Ok(a) => a,
        Err(e) => {
            if as_json {
                println!("{}", serde_json::json!({"ok": false, "error": e}));
            } else {
                eprintln!("collab: {e}");
            }
            std::process::exit(1);
        }
    };
    let newer = av.manifest.version != VERSION;
    if as_json {
        println!(
            "{}",
            serde_json::json!({
                "ok": true, "current": VERSION, "available": av.manifest.version,
                "newer": newer, "notes": av.manifest.notes,
            })
        );
        if !install_it {
            return;
        }
    } else {
        println!("running   {VERSION}");
        println!("available {}  (signature checks out)", av.manifest.version);
        if !av.manifest.notes.is_empty() {
            println!("\n{}\n", av.manifest.notes);
        }
    }
    if !newer {
        if !as_json {
            println!("already up to date");
        }
        return;
    }
    if !install_it {
        println!("run `collab update -yes` to install it");
        return;
    }
    let staged = match download(&av).and_then(|d| install(&d)) {
        Ok(d) => d,
        Err(e) => {
            if as_json {
                println!("{}", serde_json::json!({"ok": false, "error": e}));
            } else {
                eprintln!("collab: {e}");
            }
            std::process::exit(1);
        }
    };
    if as_json {
        println!("{}", serde_json::json!({"ok": true, "installed": staged}));
    } else {
        println!("installed:");
        for d in staged {
            println!("  {d}");
        }
        println!("\nrestart the server and the app to pick it up:");
        println!("  launchctl kickstart -k gui/$(id -u)/com.tankun.collab");
    }
}
