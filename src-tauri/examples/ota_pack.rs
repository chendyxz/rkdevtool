use std::path::Path;

use rkdevtool_lib::ota::pack_ota_zip;

fn main() {
    let args: Vec<String> = std::env::args().collect();
    if args.len() != 6 {
        eprintln!("usage: ota_pack <apk|rom> <version> <pk> <payload> <output.zip>");
        std::process::exit(2);
    }
    let result = pack_ota_zip(
        &args[1],
        &args[2],
        &args[3],
        Path::new(&args[4]),
        Path::new(&args[5]),
        |text| println!("{text}"),
    );
    match result {
        Ok(path) => println!("{path}"),
        Err(error) => {
            eprintln!("{error}");
            std::process::exit(1);
        }
    }
}
