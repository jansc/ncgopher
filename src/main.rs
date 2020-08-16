extern crate clap;
#[macro_use]
extern crate log;
extern crate base64;
extern crate config;
extern crate dirs;
extern crate regex;
extern crate rusqlite;
extern crate serde;
extern crate serde_derive;
extern crate sha2;
extern crate textwrap;
extern crate toml;
extern crate x509_parser;

use clap::{App, Arg};
use controller::Controller;
use cursive::Cursive;
use cursive::CursiveExt;
use lazy_static::lazy_static;
use settings::Settings;
use std::io::{stdout, Write};
use std::process::exit;
use std::sync::RwLock;
use url::Url;

mod bookmarks;
mod certificates;
mod controller;
mod gemini;
mod gophermap;
mod history;
mod ncgopher;
mod settings;
mod ui;

lazy_static! {
    static ref SETTINGS: RwLock<Settings> = RwLock::new(match Settings::new() {
        Ok(settings) => {
            settings
        }
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
        .arg(
            Arg::with_name("debug")
                .short("d")
                .long("debug")
                .value_name("FILE")
                .help("Enable debug logging to the specified file")
                .takes_value(true),
        )
        .arg(
            Arg::with_name("URL")
                .help("URL to open after startup")
                .index(1),
        )
        .get_matches();

    let mut homepage = Url::parse(
        SETTINGS
            .read()
            .unwrap()
            .get_str("homepage")
            .expect("Could not find homepage in config")
            .as_str(),
    )
    .unwrap();
    if let Some(url) = matches.value_of("URL") {
        match Url::parse(url) {
            Ok(url) => homepage = url,
            Err(e) => {
                panic!("Invalid URL: {} ({})", url, e);
            }
        }
    }

    let mut app = Cursive::default();
    //app.set_theme(SETTINGS.read().unwrap().get_theme());
    let theme = SETTINGS.read().unwrap().get_str("theme").unwrap();
    app.load_toml(SETTINGS.read().unwrap().get_theme_by_name(theme))
        .unwrap();
    let controller = Controller::new(app, homepage);
    match controller {
        Ok(mut controller) => controller.run(),
        Err(e) => println!("Error: {}", e),
    };
    print!("\x1B[?1002l");
    stdout().flush().expect("could not flush stdout");
    pancurses::endwin();
}
