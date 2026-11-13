fn main() -> anyhow::Result<()> {
    env_logger::Builder::from_env(env_logger::Env::default().default_filter_or("warn")).init();
    let mut args = std::env::args_os().skip(1);
    match (args.next(), args.next()) {
        (None, None) => dockspace_gpui_harness::run(),
        (Some(arg), None) if arg == "--smoke" => dockspace_gpui_harness::run_smoke(),
        (Some(arg), None) if arg == "--smoke-close" => dockspace_gpui_harness::run_smoke_close(),
        _ => anyhow::bail!("usage: gpui-workbench [--smoke | --smoke-close]"),
    }
    Ok(())
}
