mod app;
mod config;
mod event;
mod modal;
mod tree_state;
mod ui;
mod util;

use std::collections::{HashMap, HashSet};
use std::io;
use std::path::PathBuf;
use std::time::Duration;

use clap::Parser;
use crossterm::execute;
use crossterm::terminal::{
    EnterAlternateScreen, LeaveAlternateScreen, disable_raw_mode, enable_raw_mode,
};
use ratatui::Terminal;
use ratatui::backend::CrosstermBackend;

use app::App;
use config::{CliArgs, load_config_file, merge_config};
use modal::{MibFileEntry, MibFileStatus};

fn main() -> io::Result<()> {
    let cli = CliArgs::parse();
    let file_config = load_config_file();
    let app_config = merge_config(file_config, &cli);

    // Load MIBs
    let (oid_tree, mib_files) = load_mibs(&app_config);

    // Install panic hook BEFORE terminal setup so it can restore even
    // if setup itself panics
    let original_hook = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |panic_info| {
        let _ = restore_terminal();
        original_hook(panic_info);
    }));

    // Setup terminal
    let mut terminal = setup_terminal()?;

    // Create tokio runtime for SNMP worker
    let runtime = tokio::runtime::Runtime::new().map_err(io::Error::other)?;

    // Run application within tokio context
    let result =
        runtime.block_on(async { run(&mut terminal, oid_tree, mib_files, &app_config).await });

    // Restore terminal
    restore_terminal()?;

    result
}

fn setup_terminal() -> io::Result<Terminal<CrosstermBackend<io::Stdout>>> {
    enable_raw_mode()?;
    let mut stdout = io::stdout();
    execute!(stdout, EnterAlternateScreen)?;
    let backend = CrosstermBackend::new(stdout);
    Terminal::new(backend)
}

fn restore_terminal() -> io::Result<()> {
    disable_raw_mode()?;
    execute!(io::stdout(), LeaveAlternateScreen)?;
    Ok(())
}

async fn run(
    terminal: &mut Terminal<CrosstermBackend<io::Stdout>>,
    oid_tree: mib_parser::OidTree,
    mib_files: Vec<MibFileEntry>,
    app_config: &config::AppConfig,
) -> io::Result<()> {
    let mut app = App::new(oid_tree, mib_files, app_config);

    // Initialize SNMP worker
    app.init_worker(app_config.debug);

    // Always show connection manager on startup
    app.open_connection_manager(true);

    while app.running {
        terminal.draw(|frame| ui::draw(frame, &mut app))?;

        // Check for SNMP responses (non-blocking)
        // Take the receiver out to avoid double-borrow of app
        if let Some(mut rx) = app.response_rx.take() {
            while let Ok(response) = rx.try_recv() {
                app.handle_snmp_response(response);
            }
            app.response_rx = Some(rx);
        }

        if let Some(msg) = event::poll_event(Duration::from_millis(100), &app) {
            app.update(msg);
        }
    }

    Ok(())
}

fn load_mibs(config: &config::AppConfig) -> (mib_parser::OidTree, Vec<MibFileEntry>) {
    let bundled_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("..")
        .join("mib-parser")
        .join("mibs");

    // Collect (path, is_bundled) pairs — with canonical-path dedup.
    let mut path_infos: Vec<(PathBuf, bool)> = Vec::new();
    let mut seen_canonical: HashSet<PathBuf> = HashSet::new();

    let mut add_path = |path: PathBuf, is_bundled: bool| {
        let canonical = path.canonicalize().unwrap_or_else(|_| path.clone());
        if seen_canonical.insert(canonical) {
            path_infos.push((path, is_bundled));
        }
    };

    // Bundled MIBs
    if bundled_dir.exists() {
        let bundled_files = [
            "SNMPv2-SMI.txt",
            "SNMPv2-TC.txt",
            "SNMPv2-CONF.txt",
            "SNMPv2-MIB.txt",
            "IF-MIB.txt",
            "IANAifType-MIB.txt",
            "IP-MIB.txt",
            "IP-FORWARD-MIB.txt",
            "TCP-MIB.txt",
            "UDP-MIB.txt",
            "HOST-RESOURCES-MIB.txt",
            "HOST-RESOURCES-TYPES.txt",
            "SNMP-FRAMEWORK-MIB.txt",
            "IANA-RTPROTO-MIB.txt",
        ];
        for name in &bundled_files {
            let path = bundled_dir.join(name);
            if path.exists() {
                add_path(path, true);
            }
        }
    }

    // System MIB directories
    let system_mib_dirs = ["/usr/share/snmp/mibs", "/usr/local/share/snmp/mibs"];
    for dir in &system_mib_dirs {
        let dir = PathBuf::from(dir);
        if let Ok(entries) = std::fs::read_dir(&dir) {
            for entry in entries.flatten() {
                let path = entry.path();
                if path.is_file() {
                    add_path(path, false);
                }
            }
        }
    }

    // Config directories
    for dir in &config.mib_dirs {
        if let Ok(entries) = std::fs::read_dir(dir) {
            for entry in entries.flatten() {
                let path = entry.path();
                if path.is_file() {
                    add_path(path, false);
                }
            }
        }
    }

    // Individual config files
    for file in &config.mib_files {
        if file.exists() {
            add_path(file.clone(), false);
        }
    }

    let paths: Vec<PathBuf> = path_infos.iter().map(|(p, _)| p.clone()).collect();
    let (all_modules, warnings) = mib_parser::load_mibs_tolerant(&paths);

    for warning in &warnings {
        if config.debug {
            util::debug_log_warning(warning);
        }
    }

    // Build error map from warning messages.
    let mut error_map: HashMap<String, MibFileStatus> = HashMap::new();
    for warning in &warnings {
        if let Some(rest) = warning.strip_prefix("Failed to read ")
            && let Some(colon_pos) = rest.find(": ")
        {
            error_map.insert(
                rest[..colon_pos].to_string(),
                MibFileStatus::ReadError(rest[colon_pos + 2..].to_string()),
            );
        } else if let Some(rest) = warning.strip_prefix("Skipping ")
            && let Some(parse_pos) = rest.find(" (parse error): ")
        {
            error_map.insert(
                rest[..parse_pos].to_string(),
                MibFileStatus::ParseError(rest[parse_pos + 16..].to_string()),
            );
        }
    }

    // Build per-path module info from successfully parsed modules.
    let mut path_modules: HashMap<String, (Vec<String>, usize)> = HashMap::new();
    for module in &all_modules {
        let entry = path_modules
            .entry(module.source_file.clone())
            .or_insert((Vec::new(), 0));
        if !entry.0.contains(&module.name) {
            entry.0.push(module.name.clone());
        }
        entry.1 += module.objects.len();
    }

    // Build MibFileEntry list.
    let mib_file_entries: Vec<MibFileEntry> = path_infos
        .iter()
        .map(|(path, is_bundled)| {
            let path_str = path.display().to_string();
            if let Some(err_status) = error_map.get(&path_str) {
                MibFileEntry {
                    path: path.clone(),
                    modules: Vec::new(),
                    object_count: 0,
                    status: err_status.clone(),
                    is_bundled: *is_bundled,
                }
            } else if let Some((modules, count)) = path_modules.get(&path_str) {
                MibFileEntry {
                    path: path.clone(),
                    modules: modules.clone(),
                    object_count: *count,
                    status: MibFileStatus::Loaded,
                    is_bundled: *is_bundled,
                }
            } else {
                MibFileEntry {
                    path: path.clone(),
                    modules: Vec::new(),
                    object_count: 0,
                    status: MibFileStatus::ParseError("Unknown error".to_string()),
                    is_bundled: *is_bundled,
                }
            }
        })
        .collect();

    let tree = match mib_parser::build_tree_from_modules(&all_modules) {
        Ok(tree) => tree,
        Err(e) => {
            if config.debug {
                util::debug_log_warning(&format!("Failed to build MIB tree: {}", e));
            }
            mib_parser::OidTree::new()
        }
    };

    (tree, mib_file_entries)
}
