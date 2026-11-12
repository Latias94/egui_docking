fn main() -> anyhow::Result<()> {
    let mut args = std::env::args_os().skip(1);
    match (args.next(), args.next()) {
        (None, None) => dockspace_gpui_harness::run(),
        (Some(arg), None) if arg == "--smoke" => dockspace_gpui_harness::run_smoke(),
        _ => anyhow::bail!("usage: gpui-workbench [--smoke]"),
    }
    Ok(())
}
