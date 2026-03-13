mod app;
mod config;
mod event;
mod modal;
mod tree_state;
mod ui;

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
use config::{CliArgs, load_config_file, merge_config, to_snmp_config};

fn main() -> io::Result<()> {
    let cli = CliArgs::parse();
    let file_config = load_config_file();
    let app_config = merge_config(file_config, &cli);

    // Load MIBs
    let oid_tree = load_mibs(&app_config);

    // Build SNMP config if host is provided (for auto-connect)
    let snmp_config = to_snmp_config(&app_config);

    // Setup terminal
    let mut terminal = setup_terminal()?;

    // Install panic hook that restores terminal before printing panic
    let original_hook = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |panic_info| {
        let _ = restore_terminal();
        original_hook(panic_info);
    }));

    // Create tokio runtime for SNMP worker
    let runtime = tokio::runtime::Runtime::new().map_err(io::Error::other)?;

    // Run application within tokio context
    let result =
        runtime.block_on(async { run(&mut terminal, oid_tree, snmp_config, &app_config).await });

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
    snmp_config: Option<snmp_client::SnmpConfig>,
    app_config: &config::AppConfig,
) -> io::Result<()> {
    let mut app = App::new(oid_tree);

    // Pre-fill connect modal defaults from config
    app.connect_host = app_config.host.clone().unwrap_or_default();
    app.connect_port = app_config.port;
    app.connect_version = app_config.snmp_version.clone();
    app.connect_community = app_config.community.clone();

    // Initialize SNMP worker
    app.init_worker(app_config.debug);

    // Auto-connect if host was provided via CLI/config
    if let Some(config) = snmp_config {
        app.connect(config);
    }

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

fn load_mibs(config: &config::AppConfig) -> mib_parser::OidTree {
    let bundled_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("..")
        .join("mib-parser")
        .join("mibs");

    let mut mib_paths: Vec<PathBuf> = Vec::new();

    // Load bundled MIBs
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
                mib_paths.push(path);
            }
        }
    }

    // Load MIBs from standard system directories (Linux)
    let system_mib_dirs = ["/usr/share/snmp/mibs", "/usr/local/share/snmp/mibs"];
    for dir in &system_mib_dirs {
        let dir = PathBuf::from(dir);
        if let Ok(entries) = std::fs::read_dir(&dir) {
            for entry in entries.flatten() {
                let path = entry.path();
                if path.is_file() {
                    mib_paths.push(path);
                }
            }
        }
    }

    // Load MIBs from config directories
    for dir in &config.mib_dirs {
        if let Ok(entries) = std::fs::read_dir(dir) {
            for entry in entries.flatten() {
                let path = entry.path();
                if path.is_file() {
                    mib_paths.push(path);
                }
            }
        }
    }

    // Load individual MIB files from config
    for file in &config.mib_files {
        if file.exists() {
            mib_paths.push(file.clone());
        }
    }

    match mib_parser::load_mibs(&mib_paths) {
        Ok(tree) => tree,
        Err(e) => {
            eprintln!("Warning: Failed to load MIBs: {}", e);
            mib_parser::OidTree::new()
        }
    }
}
