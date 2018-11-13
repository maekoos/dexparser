// TODO: remove me
#![feature(trace_macros)]

#[macro_use] extern crate nom;
extern crate leb128;

mod parser;

use std::fs::File;
use std::io::Read;

fn main() {

    let mut file = File::open("/home/michael/devel/dexparser/classes.dex").unwrap();

    let mut dex_file = Vec::new();

    file.read_to_end(&mut dex_file);

    match parser::parse(&dex_file) {
        Ok(res) => println!("Result: {:?}", res),
        Err(e) => println!("ERROR!: {:#?}", e)
    }
}