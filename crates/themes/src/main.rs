// SPDX-License-Identifier: GPL-3.0-or-later
mod cli;
mod gui;
mod service;
mod tokens_cli;

fn main() {
    let args: Vec<String> = std::env::args().collect();
    let argv0 = std::path::Path::new(&args[0])
        .file_name()
        .map(|s| s.to_string_lossy().into_owned())
        .unwrap_or_default();
    let code = if args.iter().skip(1).any(|a| a == "--dbus-service") {
        service::run()
    } else if args.len() == 1 && argv0 != "theme" {
        gui::run(gui::GuiOptions::default())
    } else {
        cli::run(args)
    };
    std::process::exit(code);
}
