use std::fs::{self, OpenOptions};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::{self, Command};

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
        [command] if command == "update" => update_pin(&pin_dir()?),
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
    println!("  nixforge init        Generate missing Nix environment files");
    println!("  nixforge update      Refresh the shared flake pin used by future inits");
    println!("  nixforge --help      Show this help message");
    println!("  nixforge --version   Show version information");
    println!();
    println!("Files are only generated when they do not already exist.");
    println!("The shared flake pin lives in $XDG_DATA_HOME/nixforge (default ~/.local/share/nixforge).");
}

fn init_in_dir(dir: &Path, pin_dir: &Path) -> Result<InitReport, String> {
    let shared_lock_path = pin_dir.join("flake.lock");
    let flake_lock = match fs::read_to_string(&shared_lock_path) {
        Ok(contents) => contents,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => FLAKE_LOCK_TEMPLATE.to_string(),
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

    Ok(InitReport { statuses })
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

fn update_pin(pin_dir: &Path) -> Result<(), String> {
    fs::create_dir_all(pin_dir).map_err(|error| format!("failed to create `{}`: {error}", pin_dir.display()))?;

    let flake_path = pin_dir.join("flake.nix");
    fs::write(&flake_path, FLAKE_TEMPLATE)
        .map_err(|error| format!("failed to write `{}`: {error}", flake_path.display()))?;

    let lock_path = pin_dir.join("flake.lock");
    write_if_missing(&lock_path, FLAKE_LOCK_TEMPLATE)
        .map_err(|error| format!("failed to create `{}`: {error}", lock_path.display()))?;

    let status = Command::new("nix")
        .args(["flake", "update"])
        .current_dir(pin_dir)
        .status()
        .map_err(|error| format!("failed to run `nix flake update`: {error}"))?;

    if !status.success() {
        return Err(format!("`nix flake update` failed ({status})"));
    }

    println!("Updated shared flake pin: {}", lock_path.display());
    println!("New projects initialized by `nixforge init` will use this lock.");
    Ok(())
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
            }
        );
        assert_eq!(read(&temp.path.join("flake.nix")), FLAKE_TEMPLATE);
        assert_eq!(read(&temp.path.join("flake.lock")), FLAKE_LOCK_TEMPLATE);
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

        assert!(report.statuses.contains(&("flake.lock", FileStatus::Generated)));
        assert_eq!(read(&temp.path.join("flake.lock")), "pinned lock");
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
        assert_eq!(read(&temp.path.join("flake.nix")), before_flake);
        assert_eq!(read(&temp.path.join("flake.lock")), before_lock);
    }

    #[test]
    fn rejects_unknown_command() {
        let error = run(&["unknown".to_string()]).expect_err("unknown command should fail");
        assert!(error.contains("unknown command `unknown`"));
    }
}
