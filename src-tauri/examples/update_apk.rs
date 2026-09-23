use std::path::Path;

use rkdevtool_lib::apk_update::update_firmware_apk_with_tools;

fn main() {
    let args: Vec<String> = std::env::args().collect();
    if args.len() != 5 {
        eprintln!("usage: update_apk <firmware.img> <app.apk> <output.img> <tools-dir>");
        std::process::exit(2);
    }
    match update_firmware_apk_with_tools(
        Path::new(&args[1]),
        Path::new(&args[2]),
        Path::new(&args[3]),
        Path::new(&args[4]),
    ) {
        Ok(message) => println!("{message}"),
        Err(error) => {
            eprintln!("{error}");
            std::process::exit(1);
        }
    }
}
