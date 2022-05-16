use serde::{Serialize, Deserialize, Deserializer};
use toml::Value;
use std::collections::HashMap;
use std::env;
use std::fs::{self, DirBuilder, File as FsFile};
use std::io::Write;
use std::path::{Path, PathBuf};
//use cursive::theme::{Theme, BorderStyle};
//use cursive::theme::BaseColor::*;
//use cursive::theme::Color::*;
//use cursive::theme::PaletteColor::*;

pub struct Settings {
    pub config: NewConfig,
    config_filename: String,
    themes: HashMap<String, String>,
}

#[derive(Serialize, Deserialize, Debug)]
pub struct NewConfig {
    #[serde(default = "default_download_path")]
    #[serde(deserialize_with = "ok_or_default")]
    pub download_path: String,
    #[serde(default = "default_homepage")]
    #[serde(deserialize_with = "ok_or_default")]
    pub homepage: String,
    #[serde(default = "default_debug")]
    #[serde(deserialize_with = "ok_or_default")]
    pub debug: String,
    #[serde(default = "default_theme")]
    #[serde(deserialize_with = "ok_or_default")]
    pub theme: String,
    #[serde(default = "default_html_command")]
    #[serde(deserialize_with = "ok_or_default")]
    pub html_command: String,
    #[serde(default = "default_image_command")]
    #[serde(deserialize_with = "ok_or_default")]
    pub image_command: String,
    #[serde(default = "default_telnet_command")]
    #[serde(deserialize_with = "ok_or_default")]
    pub telnet_command: String,
    #[serde(default = "default_textwrap")]
    #[serde(deserialize_with = "ok_or_default")]
    pub textwrap: String
}

fn ok_or_default<'a, T, D>(deserializer: D) -> Result<T, D::Error>
    where T: Deserialize<'a> + Default,
                    D: Deserializer<'a>
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
    } else if let Ok(cwd) = env::current_exe() {
        Some(cwd)
    } else {
        None
    };

    if let Some(dl_dir) = dl_dir {
        DirBuilder::new().recursive(true).create(&dl_dir).ok(); // Continue on failure.
        return dl_dir.into_os_string().into_string().unwrap_or_default();
    }
    return String::new();
}

fn default_homepage() -> String { "about:help".to_owned() }
fn default_debug() -> String { "false".to_owned() }
fn default_theme() -> String { "lightmode".to_owned() }
fn default_html_command() -> String { "".to_owned() }
fn default_image_command() -> String { "".to_owned() }
fn default_telnet_command() -> String { "".to_owned() }
fn default_textwrap() -> String { "80".to_owned() }

impl Settings {
    pub fn new() -> Settings {
        // Create config dir if necessary
        match dirs::config_dir() {
            Some(mut dir) => {
                dir.push(env!("CARGO_PKG_NAME"));
                let dir = dir.into_os_string().into_string().unwrap();
                if !Path::new(&dir).exists() {
                    match fs::create_dir_all(dir) {
                        Err(why) => warn!("Could not create config dir: {}", why),
                        Ok(()) => (),
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
            config_filename: config_filename,
            themes: themes
        }
    }

    pub fn write_settings_to_file(&mut self) -> std::io::Result<()> {
        let filename = self.config_filename.clone();
        info!("Saving settings to file: {}", filename);
        // Create a path to the desired file
        let path = Path::new(&filename);

        let mut file = match FsFile::create(&path) {
            Err(why) => return Err(why),
            Ok(file) => file,
        };

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
