extern crate clap;
#[macro_use]
extern crate log;
extern crate dirs;
extern crate config;
extern crate serde;

extern crate serde_derive;
extern crate toml;


use clap::{App, Arg};
use cursive::Cursive;
use url::Url;
use std::sync::RwLock;
use std::process::exit;
use controller::Controller;
use settings::Settings;
use lazy_static::lazy_static;

mod ncgopher;
mod controller;
mod gophermap;
mod history;
mod bookmarks;
mod ui;
mod settings;
mod traits;


lazy_static! {
    static ref SETTINGS: RwLock<Settings> = RwLock::new(match Settings::new() {
        Ok(settings) => {settings},
        Err(e) => {
            println!("Could not read settings: {}", e);
            exit(1);
        }
    });
}

fn main() {
    let app_name = env!("CARGO_PKG_NAME");
    let matches = App::new(app_name)
        .version(env!("CARGO_PKG_VERSION"))
        .author("Jan Schreiber <jan@mecinus.com>")
        .about("An ncurses gopher client for the modern internet")
        .arg(Arg::with_name("debug")
                .short("d")
                .long("debug")
                .value_name("FILE")
                .help("Enable debug logging to the specified file")
                .takes_value(true),
        )
        .arg(Arg::with_name("URL")
             .help("URL to open after startup")
             .index(1))
        .get_matches();

    let mut homepage = Url::parse(SETTINGS.read().unwrap().get_str("homepage").expect("Could not find homepage in config").as_str()).unwrap();
    ////let mut homepage = Url::parse(settings.homepage.url.as_str()).unwrap();
    if let Some(url) = matches.value_of("URL") {
        match Url::parse(url) {
            Ok(url) => homepage = url,
            Err(e) => {
                panic!("Invalid URL: {} ({})", url, e);
            }
        }
    }

    let app = Cursive::default();
    let controller = Controller::new(app, homepage);
    match controller {
        Ok(mut controller) => controller.run(),
        Err(e) => println!("Error: {}", e),
    };
}

