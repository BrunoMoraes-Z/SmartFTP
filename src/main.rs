// Hide the console window for the GUI build on Windows (stdio MCP mode still works when piped).
#![cfg_attr(all(windows, not(debug_assertions)), windows_subsystem = "windows")]

mod core;
mod i18n;
mod mcp;
mod platform;
mod ui;
mod update;

fn main() -> anyhow::Result<()> {
    tracing_subscriber::fmt()
        .with_writer(std::io::stderr)
        .with_env_filter(tracing_subscriber::EnvFilter::try_from_default_env().unwrap_or_else(|_| "smartftp=info,warn".into()))
        .init();
    let rt = tokio::runtime::Builder::new_multi_thread().enable_all().worker_threads(4).build()?;
    let core = core::Core::new(rt.handle().clone());

    if std::env::args().any(|a| a == "--mcp-stdio") {
        return rt.block_on(mcp::serve_stdio(core));
    }

    mcp::ensure_http(core.clone());
    ui::run(core);
    rt.shutdown_background();
    Ok(())
}
