use std::fs::{self, OpenOptions};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::{self, Command, Stdio};

const FLAKE_TEMPLATE: &str = include_str!("templates/flake.nix");
const FLAKE_LOCK_TEMPLATE: &str = include_str!("../flake.lock");
const ENVRC_TEMPLATE: &str = include_str!("templates/envrc");
const JUSTFILE_TEMPLATE: &str = include_str!("templates/justfile");
const GITIGNORE_TEMPLATE: &str = include_str!("templates/gitignore");

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum FileStatus {
    Generated,
    Skipped,
}

impl FileStatus {
    fn label(self) -> &'static str {
        match self {
            Self::Generated => "generated",
            Self::Skipped => "skipped (already exists)",
        }
    }
}

#[derive(Debug, PartialEq, Eq)]
struct InitReport {
    statuses: Vec<(&'static str, FileStatus)>,
    created_pin: Option<PathBuf>,
}

impl InitReport {
    fn generated_any(&self) -> bool {
        self.statuses
            .iter()
            .any(|(_, status)| *status == FileStatus::Generated)
    }

    fn print(&self) {
        for (name, status) in &self.statuses {
            println!("{name}: {}", status.label());
        }

        if let Some(path) = &self.created_pin {
            println!();
            println!("Shared flake pin created: {}", path.display());
            println!("Run `nixforge update` to refresh it for future inits.");
        }

        if self.generated_any() {
            println!();
            println!("Next steps:");
            println!("  direnv allow   # if you use direnv");
            println!("  nix develop    # enter the development shell");
            println!("  just --list    # show Nix-related commands");
        } else {
            println!();
            println!("No files generated. All template files already exist.");
        }
    }
}

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();

    if let Err(error) = run(&args) {
        eprintln!("error: {error}");
        process::exit(1);
    }
}

fn run(args: &[String]) -> Result<(), String> {
    match args {
        [] => Err("missing command; try `nixforge --help`".to_string()),
        [command] if command == "init" => {
            let current_dir =
                std::env::current_dir().map_err(|error| format!("failed to get current directory: {error}"))?;
            init_in_dir(&current_dir, &pin_dir()?)?.print();
            Ok(())
        }
        [command, rest @ ..] if command == "pin" => run_pin(&pin_dir()?, rest),
        [command] if command == "update" => update_pin(&pin_dir()?),
        [command, ..] if command == "update" => Err("usage: nixforge update".to_string()),
        [flag] if flag == "--help" || flag == "-h" => {
            print_help();
            Ok(())
        }
        [flag] if flag == "--version" || flag == "-V" => {
            println!("nixforge {}", env!("CARGO_PKG_VERSION"));
            Ok(())
        }
        [command, ..] => Err(format!("unknown command `{command}`; try `nixforge --help`")),
    }
}

fn print_help() {
    println!("nixforge {}", env!("CARGO_PKG_VERSION"));
    println!("Initialize a minimal Nix flake development environment.");
    println!();
    println!("Usage:");
    println!("  nixforge init           Generate missing Nix environment files");
    println!("  nixforge pin [LOCK]     Set the shared pin from a flake.lock (default ./flake.lock)");
    println!("  nixforge pin --rev REV  Set the shared pin to a specific nixpkgs revision");
    println!("  nixforge update         Refresh the shared pin to the latest nixpkgs");
    println!("  nixforge --help         Show this help message");
    println!("  nixforge --version      Show version information");
    println!();
    println!("Files are only generated when they do not already exist.");
    println!("The shared flake pin lives in $XDG_DATA_HOME/nixforge (default ~/.local/share/nixforge).");
}

fn init_in_dir(dir: &Path, pin_dir: &Path) -> Result<InitReport, String> {
    let shared_lock_path = pin_dir.join("flake.lock");
    let mut created_pin = None;
    let flake_lock = match fs::read_to_string(&shared_lock_path) {
        Ok(contents) => contents,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            fs::create_dir_all(pin_dir)
                .map_err(|error| format!("failed to create `{}`: {error}", pin_dir.display()))?;
            if write_if_missing(&shared_lock_path, FLAKE_LOCK_TEMPLATE)
                .map_err(|error| format!("failed to create `{}`: {error}", shared_lock_path.display()))?
                == FileStatus::Generated
            {
                created_pin = Some(shared_lock_path.clone());
            }
            FLAKE_LOCK_TEMPLATE.to_string()
        }
        Err(error) => {
            return Err(format!(
                "failed to read shared flake lock `{}`: {error}",
                shared_lock_path.display()
            ));
        }
    };

    let files: [(&'static str, &str); 5] = [
        ("flake.nix", FLAKE_TEMPLATE),
        ("flake.lock", &flake_lock),
        (".envrc", ENVRC_TEMPLATE),
        ("justfile", JUSTFILE_TEMPLATE),
        (".gitignore", GITIGNORE_TEMPLATE),
    ];

    let mut statuses = Vec::with_capacity(files.len());

    for (name, contents) in files {
        let path = dir.join(name);
        let status = write_if_missing(&path, contents)
            .map_err(|error| format!("failed to create `{}`: {error}", path.display()))?;
        statuses.push((name, status));
    }

    Ok(InitReport {
        statuses,
        created_pin,
    })
}

fn write_if_missing(path: &Path, contents: &str) -> Result<FileStatus, String> {
    let mut file = match OpenOptions::new().write(true).create_new(true).open(path) {
        Ok(file) => file,
        Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => {
            return Ok(FileStatus::Skipped);
        }
        Err(error) => return Err(error.to_string()),
    };

    if let Err(error) = file.write_all(contents.as_bytes()) {
        drop(file);
        let _ = fs::remove_file(path);
        return Err(error.to_string());
    }

    Ok(FileStatus::Generated)
}

fn pin_dir() -> Result<PathBuf, String> {
    let base = match std::env::var_os("XDG_DATA_HOME") {
        Some(dir) if !dir.is_empty() => PathBuf::from(dir),
        _ => {
            let home = std::env::var_os("HOME")
                .filter(|home| !home.is_empty())
                .ok_or_else(|| "neither XDG_DATA_HOME nor HOME is set".to_string())?;
            PathBuf::from(home).join(".local/share")
        }
    };

    Ok(base.join("nixforge"))
}

fn run_pin(pin_dir: &Path, args: &[String]) -> Result<(), String> {
    match args {
        [] => set_pin_from_lock(pin_dir, Path::new("flake.lock")),
        [flag, rev] if flag == "--rev" => set_pin_from_rev(pin_dir, rev),
        [flag] if flag == "--rev" => Err("`--rev` requires a nixpkgs revision".to_string()),
        [source] => set_pin_from_lock(pin_dir, Path::new(source)),
        _ => Err("usage: nixforge pin [FLAKE_LOCK | --rev REV]".to_string()),
    }
}

fn set_pin_from_lock(pin_dir: &Path, source: &Path) -> Result<(), String> {
    let source = if source.is_dir() {
        source.join("flake.lock")
    } else {
        source.to_path_buf()
    };

    let contents = fs::read_to_string(&source)
        .map_err(|error| format!("failed to read `{}`: {error}", source.display()))?;

    if !contents.contains("\"nixpkgs\"") {
        return Err(format!("`{}` has no `nixpkgs` input", source.display()));
    }

    prepare_pin_dir(pin_dir)?;

    let lock_path = pin_dir.join("flake.lock");
    fs::write(&lock_path, contents)
        .map_err(|error| format!("failed to write `{}`: {error}", lock_path.display()))?;

    run_nix(pin_dir, &["flake", "metadata", "--json"])?;

    println!("Adopted `{}`.", source.display());
    print_pin(&lock_path);
    Ok(())
}

fn set_pin_from_rev(pin_dir: &Path, rev: &str) -> Result<(), String> {
    prepare_pin_dir(pin_dir)?;

    let url = format!("github:NixOS/nixpkgs/{rev}");
    run_nix(pin_dir, &["flake", "lock", "--override-input", "nixpkgs", &url])?;

    print_pin(&pin_dir.join("flake.lock"));
    Ok(())
}

fn update_pin(pin_dir: &Path) -> Result<(), String> {
    prepare_pin_dir(pin_dir)?;
    run_nix(pin_dir, &["flake", "update"])?;

    print_pin(&pin_dir.join("flake.lock"));
    println!("New projects initialized by `nixforge init` will use this lock.");
    Ok(())
}

fn prepare_pin_dir(pin_dir: &Path) -> Result<(), String> {
    fs::create_dir_all(pin_dir).map_err(|error| format!("failed to create `{}`: {error}", pin_dir.display()))?;

    let flake_path = pin_dir.join("flake.nix");
    fs::write(&flake_path, FLAKE_TEMPLATE)
        .map_err(|error| format!("failed to write `{}`: {error}", flake_path.display()))?;
    Ok(())
}

fn run_nix(dir: &Path, args: &[&str]) -> Result<(), String> {
    let display = args.join(" ");
    let status = Command::new("nix")
        .args(args)
        .current_dir(dir)
        .stdout(Stdio::null())
        .status()
        .map_err(|error| format!("failed to run `nix {display}`: {error}"))?;

    if !status.success() {
        return Err(format!("`nix {display}` failed ({status})"));
    }

    Ok(())
}

fn print_pin(lock_path: &Path) {
    let rev = fs::read_to_string(lock_path)
        .ok()
        .and_then(|lock| lock_rev(&lock).map(str::to_owned));

    match rev {
        Some(rev) => println!("Shared flake pin: nixpkgs {rev} ({})", lock_path.display()),
        None => println!("Shared flake pin: {}", lock_path.display()),
    }
}

fn lock_rev(lock: &str) -> Option<&str> {
    let key = lock.find("\"rev\"")?;
    let rest = lock[key + "\"rev\"".len()..].trim_start();
    let rest = rest.strip_prefix(':')?.trim_start();
    let value = rest.strip_prefix('"')?;
    let end = value.find('"')?;
    Some(&value[..end])
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;
    use std::time::{SystemTime, UNIX_EPOCH};

    struct TempDir {
        path: PathBuf,
    }

    impl TempDir {
        fn new(name: &str) -> Self {
            let unique = SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .expect("system time should be after the Unix epoch")
                .as_nanos();
            let path = std::env::temp_dir().join(format!("nixforge-{name}-{unique}-{}", process::id()));
            fs::create_dir(&path).expect("test directory should be created");
            Self { path }
        }
    }

    impl Drop for TempDir {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.path);
        }
    }

    fn write(path: &Path, contents: &str) {
        fs::write(path, contents).expect("test file should be written");
    }

    fn read(path: &Path) -> String {
        String::from_utf8(fs::read(path).expect("test file should be read")).expect("test file should be UTF-8")
    }

    fn init(temp: &TempDir) -> Result<InitReport, String> {
        init_in_dir(&temp.path, &temp.path.join("pin"))
    }

    #[test]
    fn init_creates_all_missing_files() {
        let temp = TempDir::new("creates");
        let pin_lock = temp.path.join("pin").join("flake.lock");

        let report = init(&temp).expect("init should succeed");

        assert_eq!(
            report,
            InitReport {
                statuses: vec![
                    ("flake.nix", FileStatus::Generated),
                    ("flake.lock", FileStatus::Generated),
                    (".envrc", FileStatus::Generated),
                    ("justfile", FileStatus::Generated),
                    (".gitignore", FileStatus::Generated),
                ],
                created_pin: Some(pin_lock.clone()),
            }
        );
        assert_eq!(read(&temp.path.join("flake.nix")), FLAKE_TEMPLATE);
        assert_eq!(read(&temp.path.join("flake.lock")), FLAKE_LOCK_TEMPLATE);
        assert_eq!(read(&pin_lock), FLAKE_LOCK_TEMPLATE);
        assert_eq!(read(&temp.path.join(".envrc")), ENVRC_TEMPLATE);
        assert_eq!(read(&temp.path.join("justfile")), JUSTFILE_TEMPLATE);
        assert_eq!(read(&temp.path.join(".gitignore")), GITIGNORE_TEMPLATE);
    }

    #[test]
    fn init_prefers_shared_flake_lock() {
        let temp = TempDir::new("shared-lock");
        let pin = temp.path.join("pin");
        fs::create_dir(&pin).expect("pin directory should be created");
        write(&pin.join("flake.lock"), "pinned lock");

        let report = init(&temp).expect("init should succeed");

        assert!(report.created_pin.is_none());
        assert!(report.statuses.contains(&("flake.lock", FileStatus::Generated)));
        assert_eq!(read(&temp.path.join("flake.lock")), "pinned lock");
        assert_eq!(read(&pin.join("flake.lock")), "pinned lock");
    }

    #[test]
    fn init_skips_existing_files_without_modifying_them() {
        let temp = TempDir::new("skips");
        write(&temp.path.join("flake.nix"), "# existing flake");
        write(&temp.path.join("flake.lock"), "# existing lock");
        write(&temp.path.join(".envrc"), "# existing envrc");
        write(&temp.path.join("justfile"), "# existing justfile");
        write(&temp.path.join(".gitignore"), "# existing gitignore");

        let report = init(&temp).expect("init should succeed");

        assert!(report.statuses.iter().all(|(_, status)| *status == FileStatus::Skipped));
        assert_eq!(report.created_pin, Some(temp.path.join("pin").join("flake.lock")));
        assert_eq!(read(&temp.path.join("flake.nix")), "# existing flake");
        assert_eq!(read(&temp.path.join("flake.lock")), "# existing lock");
        assert_eq!(read(&temp.path.join(".envrc")), "# existing envrc");
        assert_eq!(read(&temp.path.join("justfile")), "# existing justfile");
        assert_eq!(read(&temp.path.join(".gitignore")), "# existing gitignore");
    }

    #[test]
    fn init_generates_missing_files_when_some_exist() {
        let temp = TempDir::new("partial");
        write(&temp.path.join("flake.nix"), "# keep me");

        let report = init(&temp).expect("init should succeed");

        assert_eq!(
            report,
            InitReport {
                statuses: vec![
                    ("flake.nix", FileStatus::Skipped),
                    ("flake.lock", FileStatus::Generated),
                    (".envrc", FileStatus::Generated),
                    ("justfile", FileStatus::Generated),
                    (".gitignore", FileStatus::Generated),
                ],
                created_pin: Some(temp.path.join("pin").join("flake.lock")),
            }
        );
        assert_eq!(read(&temp.path.join("flake.nix")), "# keep me");
    }

    #[test]
    fn init_is_idempotent() {
        let temp = TempDir::new("idempotent");

        init(&temp).expect("first init should succeed");
        let before_flake = read(&temp.path.join("flake.nix"));
        let before_lock = read(&temp.path.join("flake.lock"));

        let report = init(&temp).expect("second init should succeed");

        assert!(report.statuses.iter().all(|(_, status)| *status == FileStatus::Skipped));
        assert!(report.created_pin.is_none());
        assert_eq!(read(&temp.path.join("flake.nix")), before_flake);
        assert_eq!(read(&temp.path.join("flake.lock")), before_lock);
    }

    #[test]
    fn pin_rejects_lock_without_nixpkgs() {
        let temp = TempDir::new("bad-lock");
        write(&temp.path.join("flake.lock"), "{}");

        let error = set_pin_from_lock(&temp.path.join("pin"), &temp.path.join("flake.lock"))
            .expect_err("lock without nixpkgs should be rejected");

        assert!(error.contains("no `nixpkgs` input"));
        assert!(!temp.path.join("pin").exists());
    }

    #[test]
    fn extracts_locked_revision() {
        let lock = r#"{"nodes":{"nixpkgs":{"locked":{"rev":"abc123"}}}}"#;
        assert_eq!(lock_rev(lock), Some("abc123"));
        assert_eq!(lock_rev("{}"), None);
    }

    #[test]
    fn rejects_unknown_command() {
        let error = run(&["unknown".to_string()]).expect_err("unknown command should fail");
        assert!(error.contains("unknown command `unknown`"));
    }
}
