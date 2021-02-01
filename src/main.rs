extern crate backtrace;
extern crate clap;
#[macro_use]
extern crate log;
extern crate base64;
extern crate config;
extern crate dirs;
extern crate idna;
extern crate percent_encoding;
extern crate rusqlite;
extern crate serde;
extern crate serde_derive;
extern crate sha2;
extern crate toml;
extern crate x509_parser;

use clap::{App, Arg};
use controller::Controller;
use lazy_static::lazy_static;
use settings::Settings;
use std::fs::File;
use std::io::{stdout, Write};
use std::sync::RwLock;
use url::Url;

mod bookmarks;
mod certificates;
mod controller;
mod gemini;
mod gophermap;
mod history;
mod settings;
mod ui;
mod url_tools;

lazy_static! {
    static ref SETTINGS: RwLock<Settings> =
        RwLock::new(Settings::new().expect("could not read settings"));
}

struct Logger {
    file: std::sync::RwLock<File>,
}

impl Logger {
    fn new(file: File) -> Self {
        Self {
            file: std::sync::RwLock::new(file),
        }
    }
}

impl log::Log for Logger {
    fn enabled(&self, _: &log::Metadata) -> bool {
        true
    }
    fn log(&self, record: &log::Record) {
        self.file
            .write()
            .unwrap()
            .write_all(
                format!(
                    "{} [{:5}] {}\n",
                    chrono::Local::now(),
                    record.level(),
                    record.args()
                )
                .as_bytes(),
            )
            .unwrap_or(());
    }
    fn flush(&self) {
        self.file.write().unwrap().flush().unwrap_or(());
    }
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
                .help("Enable debug logging to the specified file. If the file already exists, new content will be appended.")
                .takes_value(true),
        )
        .arg(
            Arg::with_name("URL")
                .help("URL to open after startup")
                .index(1),
        )
        .get_matches();

    let homepage = matches
        .value_of("URL")
        .map(|url| Url::parse(url).unwrap_or_else(|_| panic!("Invalid URL: {}", url)))
        .unwrap_or_else(|| {
            Url::parse(
                SETTINGS
                    .read()
                    .unwrap()
                    .get_str("homepage")
                    .unwrap() // there is a default in Settings, so this will never fail
                    .as_str(),
            )
            .expect("Invalid URL for configured homepage")
        });
    if let Some(log_file) = matches.value_of("debug") {
        let file = std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(log_file)
            .expect("could not create log file");
        log::set_boxed_logger(Box::new(Logger::new(file)))
            .unwrap_or_else(|e| panic!("could not start debug logger: {}", e));
        log::set_max_level(log::LevelFilter::Trace);
        info!("new program run");
        eprintln!("logging into file {}", log_file);
    }

    // get default hook that prints to stdout
    let default_hook = std::panic::take_hook();
    // set new hook overwriting default hook
    std::panic::set_hook(Box::new(move |info| {
        // print to log file
        error!("{}\n{:?}", info, backtrace::Backtrace::new());
        // run default hook to print to stdout
        default_hook(info);
    }));

    let mut app = cursive::default();
    //app.set_theme(SETTINGS.read().unwrap().get_theme());
    let theme = SETTINGS.read().unwrap().get_str("theme").unwrap();
    app.load_toml(SETTINGS.read().unwrap().get_theme_by_name(theme))
        .unwrap();
    Controller::new(&mut app, homepage).expect("could not create controller");
    app.run();
    print!("\x1B[?1002l");
    stdout().flush().expect("could not flush stdout");
    pancurses::endwin();
}
