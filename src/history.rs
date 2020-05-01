use chrono::{DateTime, Local};
use rusqlite::{params, Connection, NO_PARAMS};
use serde::Serializer;
use std::sync::Arc;
use url::Url;

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
    sql: Arc<Connection>,
}

impl History {
    pub fn new() -> History {
        info!("Creating history object");
        let connection = Arc::new(Connection::open(History::get_history_filename()).unwrap());
        connection
            .execute(
                "CREATE TABLE IF NOT EXISTS history (
             id INTEGER PRIMARY KEY,
             title TEXT,
             url TEXT NOT NULL,
             timestmp DATETIME DEFAULT CURRENT_TIMESTAMP,
             visitedcount NUMBER NOT NULL DEFAULT 1
         )",
                NO_PARAMS,
            )
            .expect("Could not create history table");
        History {
            stack: Vec::new(),
            sql: connection,
        }
    }

    fn get_history_filename() -> String {
        let confdir: String = match dirs::config_dir() {
            Some(mut dir) => {
                dir.push(env!("CARGO_PKG_NAME"));
                dir.push("history.db");
                dir.into_os_string().into_string().unwrap()
            }
            None => String::new(),
        };
        confdir
    }

    pub fn add(&mut self, entry: HistoryEntry) {
        info!("Adding entry to history: {:?}", entry);
        self.stack.push(entry.clone());

        trace!("History::add(): checking for entry with url {}", entry.url);
        if self.sql.query_row(
            "SELECT id FROM history WHERE url=?1",
            params![&entry.url],
            |_| Ok(()),
        ).is_ok() {
            trace!("History::add(): Row exists, updating");
            let mut stmt = self
                .sql
                .prepare("UPDATE history SET visitedcount=visitedcount+1 WHERE url=?1")
                .expect("Could not update history entry");
            stmt.query(params![&entry.url.to_string()]);
        } else {
            trace!("History::add(): Adding entry");
            self.sql
                .execute(
                    "INSERT INTO history (url) values (?1)",
                    &[&entry.url.to_string()],
                )
                .expect("Could not write history entry");
        }
    }

    pub fn clear(&mut self) {
        trace!("History::clear()");
        self.stack.clear();
        self.sql
            .execute("DELETE FROM history", NO_PARAMS)
            .expect("Could not write history entry");
    }

    pub fn back(&mut self) -> Option<HistoryEntry> {
        // Removes the topmost entry from the history and returns it
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
        let mut stmt = self
            .sql
            .prepare(
                "SELECT title, url, timestmp, visitedcount FROM history ORDER BY timestmp LIMIT ?1",
            )
            .expect("Could not get history entry");
        let mut rows = stmt
            .query(params![num_items as u32])
            .expect("Could not execute query");
        while let Some(row) = rows.next().expect("Could not read rows") {
            let title: String = row.get(1).expect("Could not get title");
            let entry = HistoryEntry {
                title,
                url: row.get(1).expect("Could not get row"),
                timestamp: row.get(2).expect("Could not get row"),
                visited_count: row.get(3).expect("Could not get row"),
                position: 0,
            };
            res.push(entry);
        }
        trace!("Returning {} history entries", res.len());
        res
    }
}
