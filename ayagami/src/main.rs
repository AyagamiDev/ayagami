use std::{env, fs::File};

use ayagami::file;

fn main() {
    env_logger::init();

    let args: Vec<String> = env::args().collect();

    println!("Hello, world!");

    let mut f = File::open(&args[1]).unwrap();
    let model = file::ParsedModel::load(&mut f).unwrap();

    println!("{:#?}", model);
}
