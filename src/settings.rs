use std::env;
use std::path::Path;
use dirs;
use config::{ConfigError, Config, File};

#[derive(Debug, Deserialize)]
pub struct Homepage {
    pub url: String,
}

#[derive(Debug, Deserialize)]
pub struct Settings {
    pub debug: bool,
    pub homepage: Homepage,
}

impl Settings {
    pub fn new() -> Result<Self, ConfigError> {
        let mut s = Config::new();

        let confdir: String = match dirs::config_dir() {
            Some(mut dir) => {
                dir.push(env!("CARGO_PKG_NAME"));
                dir.push("config");
                dir.into_os_string().into_string().unwrap()
            },
            None => { String::new() }
        };
        println!("Looking for config file {}", confdir);

        // Set defaults
        s.set_default("homepage.url", "gopher://jan.bio:70/")?;
        s.set_default("debug", false)?;

        if Path::new(confdir.as_str()).exists() {
            // Start off by merging in the "default" configuration file
            match s.merge(File::with_name(confdir.as_str())) {
                Ok(_) => (),
                Err(e) => { warn!("Could not read config file: {}", e); },
            }
        }

        // Now that we're done, let's access our configuration
        println!("debug: {:?}", s.get_bool("debug"));
        println!("homepage: {:?}", s.get::<String>("homepage.url"));

        // You can deserialize (and thus freeze) the entire configuration as
        s.try_into()
    }
}
