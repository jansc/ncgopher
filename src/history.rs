use chrono::{DateTime, Local};
use config::{Config, File, FileFormat};
use serde::Serializer;
use std::cmp;
use std::fs::File as FsFile;
use std::io::Write;
use std::path::Path;
use std::sync::Arc;
use url::Url;
use rusqlite::{Connection, NO_PARAMS};

fn url_serialize<S>(url: &Url, s: S) -> Result<S::Ok, S::Error>
where
    S: Serializer,
{
    s.serialize_str(url.clone().into_string().as_str())
}

fn timestamp_serialize<S>(ts: &DateTime<Local>, s: S) -> Result<S::Ok, S::Error>
where
    S: Serializer,
{
    s.serialize_str(&ts.to_rfc2822())
}

#[derive(Clone, Debug, serde::Serialize)]
pub struct HistoryEntry {
    pub title: String,
    #[serde(serialize_with = "url_serialize")]
    pub url: Url,
    #[serde(serialize_with = "timestamp_serialize")]
    pub timestamp: DateTime<Local>,
    pub visited_count: u16,

    // Never serialized. Keep track of position
    #[serde(skip_serializing)]
    pub position: usize,
}

#[derive(Clone, Debug)]
pub struct History {
    /// Navigational stack, used for back-functionality
    pub stack: Vec<HistoryEntry>,
    /// Log of all visited gopherholes
    pub entries: Vec<HistoryEntry>,
    sql: Arc<Connection>,
}

impl History {
    pub fn new() -> History {
        let mut s = Config::new();
        let confdir = History::get_history_filename();
        if Path::new(confdir.as_str()).exists() {
            match s.merge(File::new(confdir.as_str(), FileFormat::Toml)) {
                Ok(_s) => (),
                Err(e) => {
                    warn!("Could not read history file: {}", e);
                }
            }
        }
        let mut entries = Vec::new();
        let connection = Arc::new(Connection::open_in_memory().unwrap());
        connection.execute(
            "CREATE TABLE IF NOT EXISTS history (
             id INTEGER PRIMARY KEY,
             title TEXT,
             url TEXT NOT NULL,
             timestmp DATETIME DEFAULT CURRENT_TIMESTAMP,
             visitedcount NUMBER NOT NULL DEFAULT 1
         )",
            NO_PARAMS,
        ).expect("Could not create history table");
        info!("Reading history...");
        info!("history: {:?}", s.get_array("history"));
        if let Ok(e) = s.get_array("history") {
            for value in e {
                if let Ok(v) = value.into_table() {
                    if let Ok(u) = Url::parse(v["url"].clone().into_str().unwrap().as_str()) {
                        let h = HistoryEntry {
                            url: u.clone(),
                            title: v["url"].clone().into_str().unwrap(),
                            timestamp: Local::now(),
                            visited_count: 1,
                            position: 0,
                        };
                        entries.push(h.clone());
                    }
                }
            }
        }
        History {
            stack: Vec::new(),
            entries,
            sql: connection,
        }
    }

    fn get_history_filename() -> String {
        let confdir: String = match dirs::config_dir() {
            Some(mut dir) => {
                dir.push(env!("CARGO_PKG_NAME"));
                dir.push("history");
                dir.into_os_string().into_string().unwrap()
            }
            None => String::new(),
        };
        info!("Looking for history file {}", confdir);
        confdir
    }

    pub fn add(&mut self, entry: HistoryEntry) {
        info!("Adding entry to history: {:?}", entry);
        self.stack.push(entry.clone());
        self.sql.execute(
            "INSERT INTO history (url) values (?1)",
            &[&entry.url.to_string()],
        ).expect("Could not write history entry");
        // Check if the element already exists in history
        match self.entries.iter().position(|e| e.url == entry.url) {
            Some(p) => {
                let mut item = self.entries.remove(p);
                info!("removed item {:?}", item);
                item.visited_count += 1;
                self.entries.insert(0, item);
            }
            None => {
                self.entries.insert(0, entry);
            }
        }
        match self.write_history_to_file() {
            Err(why) => warn!("Could not write history file: {}", why),
            Ok(()) => (),
        }
    }

    pub fn clear(&mut self) {
        self.stack.clear();
        self.entries.clear();
        self.sql.execute("DELETE FROM history", NO_PARAMS).expect("Could not write history entry");
        match self.write_history_to_file() {
            Err(why) => warn!("Could not write history file: {}", why),
            Ok(()) => (),
        }
    }

    pub fn back(&mut self) -> Option<HistoryEntry> {
        if self.stack.len() > 1 {
            self.stack.pop();
            let item = self.stack.pop().unwrap();
            self.stack.push(item.clone());
            return Some(item);
        }
        None
    }

    pub fn update_selected_item(&mut self, index: usize) {
        // Updates the current selection position of the history item
        // on top of the stack
        if !self.stack.is_empty() {
            let mut item = self.stack.pop().expect("Could not fetch history item");
            info!(
                "update_selected_item(): {} {} => {}",
                item.url, item.position, index
            );
            item.position = index;
            self.stack.push(item);
        }
    }

    pub fn get_latest_history(&self, num_items: usize) -> Vec<HistoryEntry> {
        let mut res = Vec::<HistoryEntry>::new();
        let count = cmp::min(num_items, self.entries.len());
        for i in 0..count {
            res.insert(0, self.entries[i].clone());
        }
        res
    }

    pub fn write_history_to_file(&mut self) -> std::io::Result<()> {
        let filename = History::get_history_filename();
        info!("Saving history to file: {}", filename);
        // Create a path to the desired file
        let path = Path::new(&filename);

        let mut file = match FsFile::create(&path) {
            Err(why) => return Err(why),
            Ok(file) => file,
        };

        if let Err(why) = file.write(b"# Automatically generated by ncgopher.\n") {
            return Err(why);
        };
        for h in self.clone().entries {
            self.sql.execute(
                "INSERT INTO history (url) values (?1)",
                &[&h.url.to_string()],
            ).expect("Could not write history entry");
            if let Err(why) = file.write(b"\n[[history]]\n") {
                return Err(why);
            };
            let item = toml::to_string(&h).unwrap();
            if let Err(why) = file.write_all(item.as_bytes()) {
                return Err(why);
            };
        }
        Ok(())
    }
}
