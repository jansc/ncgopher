use serde::{Deserialize, Deserializer, Serialize};
use std::collections::HashMap;
use std::env;
use std::fs::{self, DirBuilder, File as FsFile};
use std::io::Write;
use std::path::{Path, PathBuf};
use toml::Value;
//use cursive::theme::{Theme, BorderStyle};
//use cursive::theme::BaseColor::*;
//use cursive::theme::Color::*;
//use cursive::theme::PaletteColor::*;

pub struct Settings {
    pub config: NewConfig,
    config_filename: String,
    themes: HashMap<String, String>,
}

fn default_open_new_url() -> char {
    'g'
}
fn default_edit_current_url() -> char {
    'G'
}
fn default_navigate_back() -> char {
    'b'
}
fn default_close() -> char {
    'q'
}
fn default_save_page() -> char {
    's'
}
fn default_reload_page() -> char {
    'r'
}
fn default_show_link() -> char {
    'i'
}
fn default_add_bookmark() -> char {
    'a'
}
fn default_next_link() -> char {
    'l'
}
fn default_previous_link() -> char {
    'L'
}
fn default_move_down() -> char {
    'j'
}
fn default_move_up() -> char {
    'k'
}
fn default_search_in_text() -> char {
    '/'
}
fn default_next_search_result() -> char {
    'n'
}
fn default_previous_search_result() -> char {
    'N'
}
fn default_show_help() -> char {
    '?'
}

pub fn default_keybindings() -> KeyBindings {
    KeyBindings {
        open_new_url: default_open_new_url(),
        edit_current_url: default_edit_current_url(),
        navigate_back: default_navigate_back(),
        close: default_close(),
        save_page: default_save_page(),
        reload_page: default_reload_page(),
        show_link: default_show_link(),
        add_bookmark: default_add_bookmark(),
        next_link: default_next_link(),
        previous_link: default_previous_link(),
        move_up: default_move_up(),
        move_down: default_move_down(),
        search_in_text: default_search_in_text(),
        next_search_result: default_next_search_result(),
        previous_search_result: default_previous_search_result(),
        show_help: default_show_help(),
    }
}

#[derive(Serialize, Deserialize, Debug, Clone)]
#[serde(default = "default_keybindings")]
pub struct KeyBindings {
    #[serde(default = "default_open_new_url", deserialize_with = "ok_or_default")]
    pub open_new_url: char,
    #[serde(
        default = "default_edit_current_url",
        deserialize_with = "ok_or_default"
    )]
    pub edit_current_url: char,
    #[serde(default = "default_navigate_back", deserialize_with = "ok_or_default")]
    pub navigate_back: char,
    #[serde(default = "default_close", deserialize_with = "ok_or_default")]
    pub close: char,
    #[serde(default = "default_save_page", deserialize_with = "ok_or_default")]
    pub save_page: char,
    #[serde(default = "default_reload_page", deserialize_with = "ok_or_default")]
    pub reload_page: char,
    #[serde(default = "default_show_link", deserialize_with = "ok_or_default")]
    pub show_link: char,
    #[serde(default = "default_add_bookmark", deserialize_with = "ok_or_default")]
    pub add_bookmark: char,
    #[serde(default = "default_next_link", deserialize_with = "ok_or_default")]
    pub next_link: char,
    #[serde(default = "default_previous_link", deserialize_with = "ok_or_default")]
    pub previous_link: char,
    #[serde(default = "default_move_down", deserialize_with = "ok_or_default")]
    pub move_down: char,
    #[serde(default = "default_move_up", deserialize_with = "ok_or_default")]
    pub move_up: char,
    #[serde(default = "default_search_in_text", deserialize_with = "ok_or_default")]
    pub search_in_text: char,
    #[serde(
        default = "default_next_search_result",
        deserialize_with = "ok_or_default"
    )]
    pub next_search_result: char,
    #[serde(
        default = "default_previous_search_result",
        deserialize_with = "ok_or_default"
    )]
    pub previous_search_result: char,
    #[serde(default = "default_show_help", deserialize_with = "ok_or_default")]
    pub show_help: char,
}

#[derive(Serialize, Deserialize, Debug)]
pub struct NewConfig {
    #[serde(default = "default_download_path", deserialize_with = "ok_or_default")]
    pub download_path: String,
    #[serde(default = "default_homepage", deserialize_with = "ok_or_default")]
    pub homepage: String,
    #[serde(default = "default_debug", deserialize_with = "ok_or_default")]
    pub debug: String,
    #[serde(default = "default_theme", deserialize_with = "ok_or_default")]
    pub theme: String,
    #[serde(default = "default_html_command", deserialize_with = "ok_or_default")]
    pub html_command: String,
    #[serde(default = "default_image_command", deserialize_with = "ok_or_default")]
    pub image_command: String,
    #[serde(default = "default_telnet_command", deserialize_with = "ok_or_default")]
    pub telnet_command: String,
    #[serde(default = "default_textwrap", deserialize_with = "ok_or_default")]
    pub textwrap: String,
    #[serde(
        default = "default_disable_history",
        deserialize_with = "ok_or_default"
    )]
    pub disable_history: bool,
    #[serde(
        default = "default_disable_identities",
        deserialize_with = "ok_or_default"
    )]
    pub disable_identities: bool,

    // Option<> supports older config files that don't have this.
    pub keybindings: Option<KeyBindings>,
}

fn ok_or_default<'a, T, D>(deserializer: D) -> Result<T, D::Error>
where
    T: Deserialize<'a> + Default,
    D: Deserializer<'a>,
{
    let v: Value = Deserialize::deserialize(deserializer)?;
    Ok(T::deserialize(v).unwrap_or_default())
}

fn default_download_path() -> String {
    // Try to determine a sensible default download dir and create it if need be.
    let dl_dir = if let Ok(home) = env::var("HOME") {
        Some([&home, "Downloads"].iter().collect::<PathBuf>())
    } else if let Ok(tmp) = env::var("TMP") {
        Some(PathBuf::from(tmp))
    } else {
        env::current_exe().ok()
    };

    if let Some(dl_dir) = dl_dir {
        DirBuilder::new().recursive(true).create(&dl_dir).ok(); // Continue on failure.
        return dl_dir.into_os_string().into_string().unwrap_or_default();
    }
    String::new()
}

fn default_homepage() -> String {
    "about:help".to_owned()
}
fn default_debug() -> String {
    "false".to_owned()
}
fn default_theme() -> String {
    "lightmode".to_owned()
}
fn default_html_command() -> String {
    "".to_owned()
}
fn default_image_command() -> String {
    "".to_owned()
}
fn default_telnet_command() -> String {
    "".to_owned()
}
fn default_textwrap() -> String {
    "80".to_owned()
}
fn default_disable_history() -> bool {
    false
}
fn default_disable_identities() -> bool {
    false
}

impl Settings {
    pub fn new() -> Settings {
        // Create config dir if necessary
        match dirs::config_dir() {
            Some(mut dir) => {
                dir.push(env!("CARGO_PKG_NAME"));
                let dir = dir.into_os_string().into_string().unwrap();
                if !Path::new(&dir).exists() {
                    if let Err(why) = fs::create_dir_all(dir) {
                        warn!("Could not create config dir: {}", why)
                    }
                }
            }
            None => {
                println!("Could not determine config dir");
            }
        };

        let confdir = match dirs::config_dir() {
            Some(mut dir) => {
                dir.push(env!("CARGO_PKG_NAME"));
                dir.push("config.toml");
                dir.into_os_string().into_string().unwrap()
            }
            None => String::new(),
        };
        let config_filename = confdir.clone();
        println!("Looking for config file {}", confdir);

        let mut themes = HashMap::new();
        themes.insert(
            "darkmode".to_string(),
            include_str!("themes/darkmode.toml").to_string(),
        );
        themes.insert(
            "lightmode".to_string(),
            include_str!("themes/lightmode.toml").to_string(),
        );

        let mut config_string = String::new();
        if Path::new(confdir.as_str()).exists() {
            config_string = std::fs::read_to_string(confdir).unwrap();
        }
        let config_table: NewConfig = toml::from_str(&config_string).unwrap();

        Settings {
            config: config_table,
            config_filename,
            themes,
        }
    }

    pub fn write_settings_to_file(&mut self) -> std::io::Result<()> {
        let filename = self.config_filename.clone();
        info!("Saving settings to file: {}", filename);
        // Create a path to the desired file
        let path = Path::new(&filename);
        let mut file = FsFile::create(path)?;

        file.write_all(b"# Automatically generated by ncgopher.\n")?;

        let toml = toml::to_string(&self.config).unwrap();
        file.write_all(toml.as_bytes())
    }

    /*
    pub fn set<T>(&mut self, key: &str, value: T) -> Result<&mut Config, ConfigError>
    where
        T: Into<Value>,
    {
        self.config.set::<T>(key, value)
    }

    pub fn get_str(&self, key: &str) -> Result<String, ConfigError> {
        self.config.get_string(key)
    }
    */

    /*
    // Get custom theme. TODO: Read from config file
    pub fn get_theme(&self) -> Theme {
        let mut theme = Theme::default();
        theme.shadow = true;
        theme.borders = BorderStyle::Simple;
        theme.palette[Background] = Dark(Blue);
        theme.palette[View] = Light(Black);
        theme.palette[Primary] = Dark(Blue);
        theme.palette[Highlight] = Light(Cyan);
        theme.palette[HighlightInactive] = Dark(Cyan);
        theme.palette[TitlePrimary] = Dark(Magenta);
        /*
    Black,
    Red,
    Green,
    Yellow,
    Blue,
    Magenta,
    Cyan,
    White,

    Background,
    Shadow,
    View,
    Primary,
    Secondary,
    Tertiary,
    TitlePrimary,
    TitleSecondary,
    Highlight,
    HighlightInactive,
        */
        theme
    }
    */

    pub fn get_theme_by_name(&self, name: String) -> &str {
        self.themes[&name].as_str()
    }
}
