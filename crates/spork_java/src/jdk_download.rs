//! Download JDK…'s engine (IntelliJ's `JdkInstaller` and the model of its `JdkDownloadDialog`)
//! on the Foojay Disco API (`api.foojay.io/disco`), which lists each vendor's JDK packages by
//! version, OS and architecture with their links and checksums: the catalogue as IntelliJ's
//! dialog groups it, and the install into IntelliJ's folder (`~/Library/Java/JavaVirtualMachines`
//! on macOS, `~/.jdks` elsewhere), the archive's checksum checked before it is unpacked. The
//! install runs as a background task in the status bar, which its × cancels.
//!
//! [`download_jdk`] is the same install without the dialog: a vendor and a feature release in, a
//! JDK out, for whatever needs a JDK the machine lacks.

use std::io::Write as _;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use anyhow::{Context as _, Result};
use async_compression::futures::bufread::GzipDecoder;
use futures::io::BufReader;
use futures::{AsyncReadExt as _, FutureExt as _, StreamExt as _};
use gpui::{
    App, AppContext as _, Context, Entity, Global, InteractiveElement as _, IntoElement,
    ParentElement as _, Render, Role, SharedString, StatefulInteractiveElement as _, Styled as _,
    Subscription, Task, WeakEntity, Window, div, px,
};
use http_client::{AsyncBody, HttpClient};
use serde_json::Value;
use sha2::Digest as _;
use spork_workbench::Tokens;
use util::ResultExt as _;
use workspace::notifications::{NotificationId, simple_message_notification::MessageNotification};
use workspace::{StatusItemView, Workspace};

use crate::jdk::{self, Jdk};

const DISCO: &str = "https://api.foojay.io/disco/v3.0";

/// The variable naming another Disco API to ask (the end-to-end tests serve a recorded answer).
pub const DISCO_VARIABLE: &str = "SPORK_IDE_DISCO";

fn disco() -> String {
    std::env::var(DISCO_VARIABLE).unwrap_or_else(|_| DISCO.to_owned())
}

/// The variable that, set to anything, keeps a JDK's archive in the cache folder once it is
/// unpacked (the end-to-end tests reuse theirs from run to run).
pub const KEEP_ARCHIVES_VARIABLE: &str = "SPORK_IDE_KEEP_JDK_ARCHIVES";

/// A vendor IntelliJ's Download JDK offers, by its Disco distribution: IntelliJ's name for it
/// (`JdkProduct.packagePresentationText`) and the start of its install folder's name, both from
/// IntelliJ's JDK list. Its own runtime is left out: no JetBrains product name in the app.
#[derive(Debug, PartialEq, Eq)]
pub struct Vendor {
    pub distribution: &'static str,
    pub name: &'static str,
    pub folder: &'static str,
}

pub const VENDORS: &[Vendor] = &[
    Vendor {
        distribution: "dragonwell",
        name: "Alibaba Dragonwell (Standard)",
        folder: "dragonwell",
    },
    Vendor {
        distribution: "corretto",
        name: "Amazon Corretto",
        folder: "corretto",
    },
    Vendor {
        distribution: "zulu",
        name: "Azul Zulu Community™",
        folder: "azul",
    },
    Vendor {
        distribution: "liberica",
        name: "BellSoft Liberica JDK",
        folder: "liberica",
    },
    Vendor {
        distribution: "temurin",
        name: "Eclipse Temurin (AdoptOpenJDK HotSpot)",
        folder: "temurin",
    },
    Vendor {
        distribution: "graalvm_community",
        name: "GraalVM Community Edition",
        folder: "graalvm-ce",
    },
    Vendor {
        distribution: "semeru",
        name: "IBM Semeru (AdoptOpenJDK OpenJ9)",
        folder: "semeru",
    },
    Vendor {
        distribution: "microsoft",
        name: "Microsoft OpenJDK",
        folder: "ms",
    },
    Vendor {
        distribution: "graalvm",
        name: "Oracle GraalVM",
        folder: "graalvm-jdk",
    },
    Vendor {
        distribution: "oracle_open_jdk",
        name: "Oracle OpenJDK",
        folder: "openjdk",
    },
    Vendor {
        distribution: "sap_machine",
        name: "SAP SapMachine",
        folder: "sapmachine",
    },
];

/// IntelliJ's default choice: the newest Oracle OpenJDK (its JDK list's `default` item).
const DEFAULT_VENDOR: &str = "oracle_open_jdk";

/// The vendors in the order of IntelliJ's JDK list, whose first with a release is the one a
/// Version starts on.
const LIST_ORDER: [&str; 11] = [
    "oracle_open_jdk",
    "microsoft",
    "corretto",
    "liberica",
    "zulu",
    "sap_machine",
    "temurin",
    "semeru",
    "dragonwell",
    "graalvm_community",
    "graalvm",
];

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Archive {
    TarGz,
    Zip,
}

/// A JDK package of the catalogue.
#[derive(Clone, Debug, PartialEq)]
pub struct Package {
    /// The Disco API's id, by which it gives the package's link and checksum.
    pub id: String,
    pub vendor: &'static Vendor,
    /// As IntelliJ shows it: `21.0.5`, `1.8.0_432`.
    pub version: String,
    pub major: u32,
    /// The Disco API's: `aarch64`, `x64`.
    pub arch: String,
    pub archive: Archive,
    pub filename: String,
    pub size: u64,
}

impl Package {
    /// The architecture IntelliJ writes after the version, which it leaves out for x86-64.
    pub fn shown_arch(&self) -> Option<&str> {
        (!matches!(self.arch.as_str(), "x64" | "x86_64" | "amd64")).then_some(self.arch.as_str())
    }

    /// IntelliJ's `fullPresentationText`: "Amazon Corretto 21.0.5 (aarch64)".
    pub fn full_text(&self) -> String {
        let arch = self
            .shown_arch()
            .map(|arch| format!(" ({arch})"))
            .unwrap_or_default();
        format!("{} {}{arch}", self.vendor.name, self.version)
    }

    /// The install folder's name (IntelliJ's `installFolderName`): `corretto-21.0.5`.
    pub fn folder_name(&self) -> String {
        format!("{}-{}", self.vendor.folder, self.version)
    }

    /// IntelliJ's "Archive size: {0} MB", one decimal.
    pub fn size_text(&self) -> String {
        format!("Archive size: {:.1} MB", self.size as f64 / 1024. / 1024.)
    }
}

/// A Disco version as IntelliJ shows a JDK's: the build left out, and Java 8 as `1.8.0_432`.
fn version_text(java_version: &str, major: u32) -> String {
    let version = java_version
        .split(['+', '-'])
        .next()
        .unwrap_or(java_version);
    if major <= 8 {
        let update = version.split('.').nth(2).unwrap_or("0");
        return format!("1.{major}.0_{update}");
    }
    version.to_owned()
}

/// A version's parts, for ordering: `21.0.5` < `21.0.10`.
fn version_key(version: &str) -> Vec<u32> {
    version
        .split(['.', '_'])
        .map(|part| part.parse().unwrap_or(0))
        .collect()
}

/// The Disco API's names of the OS and the architecture this runs on.
fn platform() -> Result<(&'static str, &'static str, Archive)> {
    let os = match std::env::consts::OS {
        "macos" => "macos",
        "linux" => "linux",
        "windows" => "windows",
        other => anyhow::bail!("no JDK packages for {other}"),
    };
    let arch = match std::env::consts::ARCH {
        "aarch64" => "aarch64",
        "x86_64" => "x64",
        other => anyhow::bail!("no JDK packages for {other}"),
    };
    let archive = match os {
        "windows" => Archive::Zip,
        _ => Archive::TarGz,
    };
    Ok((os, arch, archive))
}

/// The packages of a Disco `packages` answer IntelliJ would list: general-availability JDKs of
/// its vendors, without JavaFX, each vendor's newest of each feature release.
pub fn parse_packages(json: &str) -> Result<Vec<Package>> {
    let value: Value = serde_json::from_str(json).context("reading the JDK list")?;
    let list = value
        .get("result")
        .unwrap_or(&value)
        .as_array()
        .context("the JDK list has no packages")?;
    let mut packages: Vec<Package> = Vec::new();
    for entry in list {
        let text = |key: &str| entry.get(key).and_then(Value::as_str).unwrap_or_default();
        let Some(vendor) = VENDORS
            .iter()
            .find(|vendor| vendor.distribution == text("distribution"))
        else {
            continue;
        };
        let archive = match text("archive_type") {
            "tar.gz" | "tgz" => Archive::TarGz,
            "zip" => Archive::Zip,
            _ => continue,
        };
        let wanted = text("package_type") == "jdk"
            && text("release_status") == "ga"
            && entry.get("javafx_bundled").and_then(Value::as_bool) != Some(true)
            && entry.get("directly_downloadable").and_then(Value::as_bool) != Some(false);
        let Some(major) = entry
            .get("major_version")
            .and_then(Value::as_u64)
            .and_then(|major| u32::try_from(major).ok())
        else {
            continue;
        };
        if !wanted || text("id").is_empty() {
            continue;
        }
        let package = Package {
            id: text("id").to_owned(),
            vendor,
            version: version_text(text("java_version"), major),
            major,
            arch: text("architecture").to_owned(),
            archive,
            filename: text("filename").to_owned(),
            size: entry.get("size").and_then(Value::as_u64).unwrap_or(0),
        };
        match packages
            .iter_mut()
            .find(|known| known.vendor == vendor && known.major == major)
        {
            Some(known) if version_key(&package.version) > version_key(&known.version) => {
                *known = package
            }
            Some(_) => {}
            None => packages.push(package),
        }
    }
    Ok(packages)
}

/// The JDKs to download, as IntelliJ's dialog groups them (`buildJdkDownloaderModel`).
#[derive(Clone, Debug, Default)]
pub struct Catalog {
    pub packages: Vec<Package>,
}

impl Catalog {
    /// The feature releases, newest first: the Version combo box.
    pub fn versions(&self) -> Vec<u32> {
        let mut versions: Vec<u32> = self.packages.iter().map(|package| package.major).collect();
        versions.sort_unstable_by(|a, b| b.cmp(a));
        versions.dedup();
        versions
    }

    /// The Vendor combo box of a release: the vendors that have it, by name; then, under "Other
    /// Versions", every other vendor's nearest release (the closest newer one, else the closest
    /// older), which picking switches the Version to.
    pub fn vendors(&self, major: u32) -> (Vec<&Package>, Vec<&Package>) {
        let mut included: Vec<&Package> = self
            .packages
            .iter()
            .filter(|package| package.major == major)
            .collect();
        let mut excluded: Vec<&Package> = Vec::new();
        for vendor in VENDORS {
            if included.iter().any(|package| package.vendor == vendor) {
                continue;
            }
            let of_vendor = || self.packages.iter().filter(|p| p.vendor == vendor);
            let nearest = of_vendor()
                .filter(|package| package.major >= major)
                .min_by_key(|package| package.major)
                .or_else(|| {
                    of_vendor()
                        .filter(|package| package.major < major)
                        .max_by_key(|package| package.major)
                });
            excluded.extend(nearest);
        }
        let by_name = |package: &&Package| package.vendor.name.to_lowercase();
        included.sort_by_key(by_name);
        excluded.sort_by_key(by_name);
        (included, excluded)
    }

    /// The package the dialog opens on: the newest Oracle OpenJDK, or the smallest of the newest
    /// release.
    pub fn default_package(&self) -> Option<&Package> {
        self.packages
            .iter()
            .filter(|package| package.vendor.distribution == DEFAULT_VENDOR)
            .max_by_key(|package| package.major)
            .or_else(|| {
                let newest = self.versions().first().copied()?;
                self.packages
                    .iter()
                    .filter(|package| package.major == newest)
                    .min_by_key(|package| package.size)
            })
    }

    /// The vendor a Version starts on: the first of IntelliJ's list that has it.
    pub fn default_for(&self, major: u32) -> Option<&Package> {
        LIST_ORDER
            .iter()
            .find_map(|vendor| self.find(vendor, major))
            .or_else(|| self.packages.iter().find(|package| package.major == major))
    }

    /// The package of `vendor` (a Disco distribution) for a feature release.
    pub fn find(&self, vendor: &str, major: u32) -> Option<&Package> {
        self.packages
            .iter()
            .find(|package| package.vendor.distribution == vendor && package.major == major)
    }
}

async fn get_text(http: &Arc<dyn HttpClient>, url: &str) -> Result<String> {
    let mut response = http
        .get(url, AsyncBody::default(), true)
        .await
        .with_context(|| format!("fetching {url}"))?;
    anyhow::ensure!(
        response.status().is_success(),
        "fetching {url} failed with status {}",
        response.status()
    );
    let mut text = String::new();
    response
        .body_mut()
        .read_to_string(&mut text)
        .await
        .with_context(|| format!("reading {url}"))?;
    Ok(text)
}

/// The JDKs this machine can download, from the Disco API: the newest package of each vendor
/// and feature release.
pub async fn fetch_catalog(http: &Arc<dyn HttpClient>) -> Result<Catalog> {
    let (os, arch, archive) = platform()?;
    let archive = match archive {
        Archive::TarGz => "tar.gz",
        Archive::Zip => "zip",
    };
    let lib_c = match os {
        "linux" => "&lib_c_type=glibc",
        _ => "",
    };
    let url = format!(
        "{}/packages?operating_system={os}&architecture={arch}&archive_type={archive}\
         &package_type=jdk&release_status=ga&latest=available{lib_c}",
        disco()
    );
    let packages = parse_packages(&get_text(http, &url).await?)?;
    anyhow::ensure!(
        !packages.is_empty(),
        "No JDK packages are available for download"
    );
    Ok(Catalog { packages })
}

/// IntelliJ's folder for downloaded JDKs (`JdkInstaller.defaultInstallDir`).
pub fn default_folder() -> PathBuf {
    let home = paths::home_dir();
    match std::env::consts::OS {
        "macos" => home.join("Library/Java/JavaVirtualMachines"),
        _ => home.join(".jdks"),
    }
}

/// The folder a package goes to in `folder`: its name, or with `-1`, `-2`… when that is taken.
pub fn install_dir(folder: &Path, package: &Package) -> PathBuf {
    let name = package.folder_name();
    let target = folder.join(&name);
    if !target.exists() {
        return target;
    }
    (1..)
        .map(|count| folder.join(format!("{name}-{count}")))
        .find(|dir| !dir.exists())
        .unwrap_or(target)
}

/// A path as IntelliJ's Location field shows it: the home folder as `~`.
pub fn shown_path(path: &Path) -> String {
    match path.strip_prefix(paths::home_dir()) {
        Ok(rest) => format!("~/{}", rest.display()),
        Err(_) => path.display().to_string(),
    }
}

/// The Location field's folder, or IntelliJ's message (`validateInstallDir`) for one it can't
/// install into.
pub fn validate_install_dir(text: &str) -> Result<PathBuf, &'static str> {
    let text = text.trim();
    if text.is_empty() {
        return Err("Target path is empty");
    }
    let path = match text.strip_prefix("~/") {
        Some(rest) => paths::home_dir().join(rest),
        None if text == "~" => paths::home_dir().clone(),
        None => PathBuf::from(text),
    };
    if path.is_file() {
        return Err("Target path is an existing file");
    }
    if std::fs::read_dir(&path).is_ok_and(|mut entries| entries.next().is_some()) {
        return Err("Target path is an existing non-empty directory");
    }
    Ok(path)
}

/// A checksum the vendor publishes for an archive.
#[derive(Clone, Debug, PartialEq, Eq)]
enum Checksum {
    Sha256(String),
    Sha512(String),
    Sha1(String),
}

impl Checksum {
    /// From the Disco API's `checksum_type` and value, the type told by the length when it gives
    /// none; MD5 isn't trusted.
    fn parse(kind: &str, hex: &str) -> Result<Self> {
        let hex = hex.trim().to_lowercase();
        anyhow::ensure!(
            !hex.is_empty() && hex.chars().all(|c| c.is_ascii_hexdigit()),
            "the vendor's checksum isn't one: {hex:?}"
        );
        Ok(
            match (kind.to_lowercase().replace('-', "").as_str(), hex.len()) {
                ("sha256", _) | ("", 64) => Checksum::Sha256(hex),
                ("sha512", _) | ("", 128) => Checksum::Sha512(hex),
                ("sha1", _) | ("", 40) => Checksum::Sha1(hex),
                (kind, _) => anyhow::bail!("the vendor's {kind} checksum can't verify a download"),
            },
        )
    }

    fn expected(&self) -> &str {
        match self {
            Checksum::Sha256(hex) | Checksum::Sha512(hex) | Checksum::Sha1(hex) => hex,
        }
    }

    fn hasher(&self) -> Hasher {
        match self {
            Checksum::Sha256(_) => Hasher::Sha256(sha2::Sha256::new()),
            Checksum::Sha512(_) => Hasher::Sha512(sha2::Sha512::new()),
            Checksum::Sha1(_) => Hasher::Sha1(sha1::Sha1::new()),
        }
    }
}

enum Hasher {
    Sha256(sha2::Sha256),
    Sha512(sha2::Sha512),
    Sha1(sha1::Sha1),
}

impl Hasher {
    fn update(&mut self, bytes: &[u8]) {
        match self {
            Hasher::Sha256(hasher) => hasher.update(bytes),
            Hasher::Sha512(hasher) => hasher.update(bytes),
            Hasher::Sha1(hasher) => hasher.update(bytes),
        }
    }

    fn hex(self) -> String {
        let bytes = match self {
            Hasher::Sha256(hasher) => hasher.finalize().to_vec(),
            Hasher::Sha512(hasher) => hasher.finalize().to_vec(),
            Hasher::Sha1(hasher) => hasher.finalize().to_vec(),
        };
        bytes.iter().map(|byte| format!("{byte:02x}")).collect()
    }
}

/// Where a package downloads from, and the checksum its archive must have: the Disco API's,
/// the file its checksum link names, or, for the vendors whose packages it lists without one,
/// the vendor's own (Corretto's checksum of its latest release, SapMachine's `.sha256.txt`).
async fn resolve(http: &Arc<dyn HttpClient>, package: &Package) -> Result<(String, Checksum)> {
    let info: Value =
        serde_json::from_str(&get_text(http, &format!("{}/ids/{}", disco(), package.id)).await?)
            .context("reading the package's details")?;
    let entry = info
        .get("result")
        .and_then(|result| result.get(0))
        .context("the JDK list has no details for the package")?;
    let text = |key: &str| entry.get(key).and_then(Value::as_str).unwrap_or_default();
    let url = text("direct_download_uri").to_owned();
    anyhow::ensure!(
        url.starts_with("https://"),
        "URL must use https:// protocol, but was: {url}"
    );
    let first_word = |text: &str| {
        text.split_whitespace()
            .next()
            .unwrap_or_default()
            .to_owned()
    };
    let checksum = if !text("checksum").is_empty() {
        Checksum::parse(text("checksum_type"), text("checksum"))?
    } else if !text("checksum_uri").is_empty() {
        let published = get_text(http, text("checksum_uri")).await?;
        Checksum::parse(text("checksum_type"), &first_word(&published))?
    } else {
        let (os, arch, _) = platform()?;
        let extension = match package.archive {
            Archive::TarGz => "tar.gz",
            Archive::Zip => "zip",
        };
        let checksum_url = match package.vendor.distribution {
            "corretto" => format!(
                "https://corretto.aws/downloads/latest_sha256/amazon-corretto-{}-{arch}-{os}-jdk.{extension}",
                package.major
            ),
            "sap_machine" => format!(
                "{}.sha256.txt",
                url.strip_suffix(&format!(".{extension}")).unwrap_or(&url)
            ),
            _ => anyhow::bail!(
                "{} publishes no checksum for {}",
                package.vendor.name,
                package.filename
            ),
        };
        Checksum::parse("", &first_word(&get_text(http, &checksum_url).await?))?
    };
    Ok((url, checksum))
}

/// A step of an install, for its progress.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Progress {
    Downloading { done: u64, total: u64 },
    Unpacking,
}

pub type OnProgress = Arc<dyn Fn(Progress) + Send + Sync>;

/// Where archives are downloaded to: the IDE's cache folder, where an archive kept
/// ([`KEEP_ARCHIVES_VARIABLE`]) serves the next install of the same package once its checksum
/// holds.
fn archives() -> PathBuf {
    paths::temp_dir().join("jdks")
}

fn checksum_of_file(path: &Path, checksum: &Checksum) -> Result<String> {
    let mut file = std::fs::File::open(path)?;
    let mut hasher = checksum.hasher();
    let mut buffer = vec![0; 1 << 16];
    loop {
        let read = std::io::Read::read(&mut file, &mut buffer)?;
        if read == 0 {
            break;
        }
        hasher.update(&buffer[..read]);
    }
    Ok(hasher.hex())
}

/// Downloads `url` to `file` unless it is there with the checksum, checking the checksum.
async fn download(
    http: &Arc<dyn HttpClient>,
    url: &str,
    file: &Path,
    size: u64,
    checksum: &Checksum,
    progress: &OnProgress,
) -> Result<()> {
    if file.is_file() && checksum_of_file(file, checksum)? == checksum.expected() {
        return Ok(());
    }
    if let Some(folder) = file.parent() {
        std::fs::create_dir_all(folder)?;
    }
    let mut response = http
        .get(url, AsyncBody::default(), true)
        .await
        .with_context(|| format!("downloading {url}"))?;
    anyhow::ensure!(
        response.status().is_success(),
        "downloading {url} failed with status {}",
        response.status()
    );
    let part = file.with_file_name(format!(
        "{}.part",
        file.file_name()
            .map(|name| name.to_string_lossy())
            .unwrap_or_default()
    ));
    let mut out = std::fs::File::create(&part).with_context(|| format!("creating {part:?}"))?;
    let mut hasher = checksum.hasher();
    let mut buffer = vec![0; 1 << 16];
    let (mut done, mut reported) = (0u64, 0u64);
    let body = response.body_mut();
    loop {
        let read = body
            .read(&mut buffer)
            .await
            .context("downloading the JDK")?;
        if read == 0 {
            break;
        }
        out.write_all(&buffer[..read])?;
        hasher.update(&buffer[..read]);
        done += read as u64;
        // a step a percent
        if size == 0 || done - reported >= size / 100 {
            reported = done;
            progress(Progress::Downloading { done, total: size });
        }
    }
    out.flush()?;
    drop(out);
    let actual = hasher.hex();
    if actual != checksum.expected() {
        std::fs::remove_file(&part).ok();
        anyhow::bail!(
            "Failed to verify the checksum of {url}: it is {actual}, but {} was expected. Check \
             your internet connection and try again later",
            checksum.expected()
        );
    }
    std::fs::rename(&part, file)?;
    Ok(())
}

/// The folder of an unpacked archive that is the JDK: the one holding `Contents/Home` (a macOS
/// bundle) or `bin/java`, the shallowest.
fn package_root(unpacked: &Path) -> Option<PathBuf> {
    let java = format!("java{}", std::env::consts::EXE_SUFFIX);
    let mut level = vec![unpacked.to_owned()];
    for _ in 0..4 {
        if let Some(found) = level.iter().find(|dir| {
            dir.join("Contents/Home/bin").join(&java).is_file()
                || dir.join("bin").join(&java).is_file()
        }) {
            return Some(found.clone());
        }
        level = level
            .iter()
            .filter_map(|dir| std::fs::read_dir(dir).ok())
            .flat_map(|entries| entries.flatten().map(|entry| entry.path()))
            .filter(|path| path.is_dir() && !path.is_symlink())
            .collect();
    }
    None
}

/// Installs `package` into `target`: downloads its archive, checks its checksum and unpacks it
/// there. The JDK installed.
pub async fn install(
    http: Arc<dyn HttpClient>,
    package: Package,
    target: PathBuf,
    progress: OnProgress,
) -> Result<Jdk> {
    let (url, checksum) = resolve(&http, &package).await?;
    let archive = archives().join(&package.filename);
    download(&http, &url, &archive, package.size, &checksum, &progress).await?;
    progress(Progress::Unpacking);
    let parent = target
        .parent()
        .with_context(|| format!("{target:?} has no parent folder"))?;
    std::fs::create_dir_all(parent).with_context(|| format!("creating {parent:?}"))?;
    // unpacked beside the target, then moved in: a cancelled install leaves nothing behind
    let staging = tempfile::Builder::new()
        .prefix(".jdk-download-")
        .tempdir_in(parent)
        .with_context(|| format!("creating a folder in {parent:?}"))?;
    let file = async_fs::File::open(&archive).await?;
    let unpacked = match package.archive {
        Archive::TarGz => async_tar::ArchiveBuilder::new(GzipDecoder::new(BufReader::new(file)))
            .set_preserve_mtime(false)
            .build()
            .unpack(staging.path())
            .await
            .with_context(|| format!("unpacking {}", package.filename)),
        Archive::Zip => util::archive::extract_zip(staging.path(), file).await,
    };
    // unpacked, the archive goes, as IntelliJ deletes its download
    if std::env::var_os(KEEP_ARCHIVES_VARIABLE).is_none() {
        std::fs::remove_file(&archive).log_err();
    }
    unpacked?;
    let root = package_root(staging.path())
        .with_context(|| format!("{} holds no JDK", package.filename))?;
    // an empty folder the Location named is replaced
    if target.is_dir() {
        std::fs::remove_dir(&target).with_context(|| format!("replacing {target:?}"))?;
    }
    std::fs::rename(&root, &target).with_context(|| format!("moving the JDK to {target:?}"))?;
    jdk::at_folder(&target).with_context(|| format!("{target:?} holds no JDK"))
}

/// Downloads and installs the newest JDK of `vendor` (a Disco distribution: `temurin`,
/// `corretto`…) for the `feature` release into `folder` (IntelliJ's is [`default_folder`]), in a
/// folder of its own (`temurin-21.0.5`): the same install as Download JDK…, without the dialog.
pub async fn download_jdk(
    http: Arc<dyn HttpClient>,
    vendor: &str,
    feature: u32,
    folder: &Path,
    progress: OnProgress,
) -> Result<Jdk> {
    let catalog = fetch_catalog(&http).await?;
    let package = catalog
        .find(vendor, feature)
        .with_context(|| format!("no JDK {feature} of {vendor} to download"))?
        .clone();
    let target = install_dir(folder, &package);
    install(http, package, target, progress).await
}

/// The JDK installs running, which the status bar shows.
#[derive(Default)]
pub(crate) struct Downloads {
    running: Vec<Running>,
    next_id: usize,
}

struct Running {
    id: usize,
    /// IntelliJ's indicator text: "Installing Amazon Corretto 21.0.5…".
    text: SharedString,
    progress: Option<Progress>,
    _task: Task<()>,
}

struct GlobalDownloads(Entity<Downloads>);

impl Global for GlobalDownloads {}

fn downloads(cx: &mut App) -> Entity<Downloads> {
    if let Some(downloads) = cx.try_global::<GlobalDownloads>() {
        return downloads.0.clone();
    }
    let downloads = cx.new(|_| Downloads::default());
    cx.set_global(GlobalDownloads(downloads.clone()));
    downloads
}

/// Installs `package` into `target` as a background task in the status bar; the JDK goes into
/// the SDK table and to `on_done` (the dialog's opener: the project whose JDK it becomes). A
/// failure is IntelliJ's "Download JDK" error.
pub(crate) fn start(
    package: Package,
    target: PathBuf,
    on_done: Box<dyn FnOnce(Jdk, &mut App)>,
    workspace: Option<WeakEntity<Workspace>>,
    cx: &mut App,
) {
    let http = cx.http_client();
    let downloads = downloads(cx);
    let text: SharedString = format!("Installing {}…", package.full_text()).into();
    downloads.update(cx, |downloads, cx| {
        let id = downloads.next_id;
        downloads.next_id += 1;
        let (sender, mut steps) = futures::channel::mpsc::unbounded();
        let on_progress: OnProgress = Arc::new(move |step| {
            sender.unbounded_send(step).ok();
        });
        let shown_target = target.clone();
        let installing = cx.background_spawn(install(http, package, target, on_progress));
        let task = cx.spawn(async move |this, cx| {
            let mut installing = installing.fuse();
            let result = loop {
                futures::select_biased! {
                    result = installing => break result,
                    step = steps.next() => {
                        if let Some(step) = step {
                            this.update(cx, |this, cx| {
                                if let Some(running) = this.running.iter_mut().find(|r| r.id == id) {
                                    running.progress = Some(step);
                                }
                                cx.notify();
                            })
                            .ok();
                        }
                    }
                }
            };
            this.update(cx, |this, cx| {
                this.running.retain(|running| running.id != id);
                cx.notify();
            })
            .ok();
            match result {
                Ok(jdk) => {
                    crate::sdks::register(&jdk).log_err();
                    cx.update(|cx| on_done(jdk, cx));
                }
                Err(error) => {
                    log::error!("downloading a JDK: {error:#}");
                    let message = format!(
                        "Failed to download JDK to {} because of {error:#}",
                        shown_path(&shown_target)
                    );
                    if let Some(workspace) = workspace.and_then(|workspace| workspace.upgrade()) {
                        workspace.update(cx, |workspace, cx| {
                            workspace.show_notification(
                                NotificationId::unique::<DownloadFailed>(),
                                cx,
                                |cx| {
                                    cx.new(|cx| {
                                        MessageNotification::new(message.clone(), cx)
                                            .with_title("Download JDK")
                                    })
                                },
                            )
                        });
                    }
                }
            }
        });
        downloads.running.push(Running {
            id,
            text,
            progress: None,
            _task: task,
        });
        cx.notify();
    });
}

struct DownloadFailed;

/// What the end-to-end tests read: the installs running, each its text and progress.
pub(crate) fn e2e_state(cx: &App) -> Value {
    let Some(downloads) = cx.try_global::<GlobalDownloads>() else {
        return Value::Array(Vec::new());
    };
    downloads
        .0
        .read(cx)
        .running
        .iter()
        .map(|running| serde_json::json!({"text": running.text.as_ref()}))
        .collect()
}

/// The status bar's background task (IntelliJ's compact `InlineProgressIndicator`): the
/// install's text, its progress bar and the × that cancels it.
pub(crate) struct DownloadStatus {
    downloads: Entity<Downloads>,
    _subscription: Subscription,
}

impl DownloadStatus {
    pub(crate) fn new(cx: &mut Context<Self>) -> Self {
        let downloads = downloads(cx);
        let _subscription = cx.observe(&downloads, |_, _, cx| cx.notify());
        Self {
            downloads,
            _subscription,
        }
    }
}

impl Render for DownloadStatus {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let Some(running) = self.downloads.read(cx).running.first() else {
            return div().into_any_element();
        };
        let (id, text) = (running.id, running.text.clone());
        // IntelliJ's text2, the step, in the tooltip as its status bar shows text alone
        let (fraction, step) = match running.progress {
            Some(Progress::Downloading { done, total }) if total > 0 => {
                (Some(done as f32 / total as f32), "Downloading…")
            }
            Some(Progress::Unpacking) => (None, "Unpacking…"),
            _ => (Some(0.), "Downloading…"),
        };
        let downloads = self.downloads.clone();
        let tokens = Tokens::get(cx);
        div()
            .id("jdk-download-progress")
            .role(Role::ProgressIndicator)
            .aria_label(text.clone())
            .flex()
            .items_center()
            .gap(px(2.))
            .text_color(tokens.text_secondary)
            .tooltip(move |window, cx| {
                gpui_kit::component::tooltip::Tooltip::new(format!("Downloading JDK… {step}"))
                    .build(window, cx)
            })
            .child(text)
            .child(
                div()
                    .px(px(4.))
                    .child(spork_workbench::controls::progress_bar(fraction, cx)),
            )
            .child(
                spork_workbench::ActionButton::new("jdk-download-cancel", "progress/stop")
                    .tooltip("Cancel")
                    .on_click(move |_, _, cx| {
                        downloads.update(cx, |downloads, cx| {
                            downloads.running.retain(|running| running.id != id);
                            cx.notify();
                        })
                    }),
            )
            .into_any_element()
    }
}

impl StatusItemView for DownloadStatus {
    fn set_active_pane_item(
        &mut self,
        _: Option<&dyn workspace::ItemHandle>,
        _: &mut Window,
        _: &mut Context<Self>,
    ) {
    }

    fn hide_setting(&self, _: &App) -> Option<workspace::HideStatusItem> {
        None
    }
}

#[cfg(test)]
mod tests {
    use super::{
        Archive, Catalog, Checksum, archives, install, package_root, parse_packages, version_text,
    };

    #[test]
    fn the_archive_goes_once_unpacked() {
        use async_zip::base::write::ZipFileWriter;
        use async_zip::{Compression, ZipEntryBuilder};
        use sha2::Digest as _;

        let archive = smol::block_on(async {
            let mut writer = ZipFileWriter::new(Vec::new());
            for (name, data) in [
                ("jdk-21.0.5+11/Contents/Home/bin/java", ""),
                (
                    "jdk-21.0.5+11/Contents/Home/release",
                    "IMPLEMENTOR=\"Eclipse Adoptium\"\nJAVA_VERSION=\"21.0.5\"\n",
                ),
            ] {
                writer
                    .write_entry_whole(
                        ZipEntryBuilder::new(name.into(), Compression::Deflate),
                        data.as_bytes(),
                    )
                    .await
                    .expect("an entry");
            }
            writer.close().await.expect("a zip")
        });
        let checksum: String = sha2::Sha256::digest(&archive)
            .iter()
            .map(|byte| format!("{byte:02x}"))
            .collect();
        let details = format!(
            r#"{{"result": [{{"direct_download_uri": "https://example.com/the-archive-goes.zip",
            "checksum": "{checksum}", "checksum_type": "sha256"}}]}}"#
        );
        let http = http_client::FakeHttpClient::create(move |request| {
            let body = match request.uri().path() {
                "/the-archive-goes.zip" => archive.clone(),
                _ => details.clone().into_bytes(),
            };
            async move {
                Ok(http_client::Response::builder()
                    .status(200)
                    .body(body.into())?)
            }
        });
        let json = r#"{"result": [{"id": "the-archive-goes", "archive_type": "zip",
            "distribution": "temurin", "major_version": 21, "java_version": "21.0.5+11",
            "release_status": "ga", "architecture": "aarch64", "package_type": "jdk",
            "filename": "the-archive-goes.zip", "size": 0}]}"#;
        let package = parse_packages(json)
            .expect("the list")
            .pop()
            .expect("a package");
        let folder = tempfile::tempdir().expect("a temporary folder");
        let jdk = smol::block_on(install(
            http,
            package,
            folder.path().join("temurin-21.0.5"),
            std::sync::Arc::new(|_| {}),
        ))
        .expect("installed");
        assert_eq!(jdk.major, 21);
        assert!(!archives().join("the-archive-goes.zip").exists());
    }

    fn entry(distribution: &str, major: u32, java_version: &str, size: u64) -> String {
        format!(
            r#"{{"id": "{distribution}{major}", "archive_type": "tar.gz", "distribution": "{distribution}",
            "major_version": {major}, "java_version": "{java_version}", "release_status": "ga",
            "architecture": "aarch64", "package_type": "jdk", "javafx_bundled": false,
            "directly_downloadable": true, "filename": "{distribution}-{java_version}.tar.gz",
            "size": {size}}}"#
        )
    }

    fn catalog() -> Catalog {
        let entries = [
            entry("temurin", 21, "21.0.5+11", 200),
            entry("temurin", 21, "21.0.4+7", 200),
            entry("temurin", 17, "17.0.13+11", 180),
            entry("corretto", 21, "21.0.5.11.1+1", 190),
            entry("oracle_open_jdk", 23, "23.0.1+11", 210),
            entry("zulu", 8, "8.0.432+6", 100),
            entry("kona", 21, "21.0.5+1", 190),
        ];
        let json = format!(r#"{{"result": [{}], "message": ""}}"#, entries.join(","));
        Catalog {
            packages: parse_packages(&json).expect("the list"),
        }
    }

    #[test]
    fn reads_the_disco_apis_packages_as_intellij_lists_them() {
        let catalog = catalog();
        // Kona isn't one of IntelliJ's vendors; Temurin 21's newest build is kept
        assert_eq!(catalog.packages.len(), 5);
        let temurin = catalog.find("temurin", 21).expect("Temurin 21");
        assert_eq!(temurin.version, "21.0.5");
        assert_eq!(temurin.archive, Archive::TarGz);
        assert_eq!(temurin.folder_name(), "temurin-21.0.5");
        assert_eq!(
            temurin.full_text(),
            "Eclipse Temurin (AdoptOpenJDK HotSpot) 21.0.5 (aarch64)"
        );
        assert_eq!(
            catalog.find("zulu", 8).map(|p| p.version.as_str()),
            Some("1.8.0_432")
        );
        assert_eq!(catalog.versions(), vec![23, 21, 17, 8]);
        assert_eq!(
            catalog.default_package().map(|p| p.vendor.distribution),
            Some("oracle_open_jdk")
        );
    }

    #[test]
    fn groups_the_vendors_of_a_version_and_the_others_nearest() {
        let catalog = catalog();
        let names = |packages: Vec<&super::Package>| {
            packages
                .iter()
                .map(|package| format!("{} {}", package.vendor.name, package.version))
                .collect::<Vec<_>>()
        };
        let (included, excluded) = catalog.vendors(21);
        assert_eq!(
            names(included),
            vec![
                "Amazon Corretto 21.0.5.11.1",
                "Eclipse Temurin (AdoptOpenJDK HotSpot) 21.0.5"
            ]
        );
        // the closest newer release, else the closest older one
        assert_eq!(
            names(excluded),
            vec!["Azul Zulu Community™ 1.8.0_432", "Oracle OpenJDK 23.0.1"]
        );
    }

    #[test]
    fn shows_versions_without_their_build() {
        assert_eq!(version_text("27.0.1.1+4", 27), "27.0.1.1");
        assert_eq!(version_text("25", 25), "25");
        assert_eq!(version_text("8.0.472+8", 8), "1.8.0_472");
    }

    #[test]
    fn takes_a_checksum_by_its_type_or_length() {
        let sha256 = "a".repeat(64);
        assert_eq!(
            Checksum::parse("", &sha256).expect("by length"),
            Checksum::Sha256(sha256.clone())
        );
        assert_eq!(
            Checksum::parse("SHA-256", &sha256.to_uppercase()).expect("by type"),
            Checksum::Sha256(sha256)
        );
        assert!(Checksum::parse("md5", &"a".repeat(32)).is_err());
        assert!(Checksum::parse("sha256", "<html>").is_err());
    }

    #[test]
    fn finds_the_jdk_in_an_unpacked_archive() {
        let unpacked = tempfile::tempdir().expect("a folder");
        let bundle = unpacked.path().join("jdk-21.0.5+11/Contents/Home/bin");
        std::fs::create_dir_all(&bundle).expect("the bundle");
        std::fs::write(bundle.join("java"), "").expect("java");
        assert_eq!(
            package_root(unpacked.path()),
            Some(unpacked.path().join("jdk-21.0.5+11"))
        );
    }

    #[test]
    fn checks_the_location_as_intellij_does() {
        let folder = tempfile::tempdir().expect("a folder");
        assert_eq!(
            super::validate_install_dir(" "),
            Err("Target path is empty")
        );
        assert_eq!(
            super::validate_install_dir(&folder.path().to_string_lossy()),
            Ok(folder.path().to_owned())
        );
        std::fs::write(folder.path().join("file"), "").expect("a file");
        assert_eq!(
            super::validate_install_dir(&folder.path().to_string_lossy()),
            Err("Target path is an existing non-empty directory")
        );
        assert_eq!(
            super::validate_install_dir(&folder.path().join("file").to_string_lossy()),
            Err("Target path is an existing file")
        );
    }
}
