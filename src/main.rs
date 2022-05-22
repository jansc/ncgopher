extern crate backtrace;
extern crate clap;
#[macro_use]
extern crate log;
extern crate base64;
extern crate dirs;
extern crate idna;
extern crate percent_encoding;
extern crate rusqlite;
extern crate serde;
extern crate serde_derive;
extern crate sha2;
extern crate toml;
extern crate x509_parser;

use clap::Parser;
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
    static ref SETTINGS: RwLock<Settings> = RwLock::new(Settings::new());
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

/// An ncurses gopher client for the modern internet
#[derive(Parser, Debug)]
#[clap(author, version, about, long_about = None)]
struct Args {
    /// Enable debug logging to the specified file. If the file already exists, new content will be appended.
    #[clap(short, long)]
    debug: Option<String>,

    /// Url to open after startup
    url: Option<String>,
}

fn main() {
    let args = Args::parse();

    let homepage = args
        .url
        .as_deref()
        .map(|url| Url::parse(url).unwrap_or_else(|_| panic!("Invalid URL: {}", url)))
        .unwrap_or_else(|| {
            Url::parse(SETTINGS.read().unwrap().config.homepage.as_str())
                .expect("Invalid URL for configured homepage")
        });
    if let Some(log_file) = args.debug.as_deref() {
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
    let theme = SETTINGS.read().unwrap().config.theme.clone();
    app.load_toml(SETTINGS.read().unwrap().get_theme_by_name(theme))
        .unwrap();
    Controller::setup(&mut app, homepage).expect("could not create controller");
    // required so async updates to the status bar get shown
    app.run();
    print!("\x1B[?1002l");
    stdout().flush().expect("could not flush stdout");
    pancurses::endwin();
}
