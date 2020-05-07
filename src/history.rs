use chrono::{DateTime, Local};
use rusqlite::{params, Connection, NO_PARAMS, Result};
use std::sync::Arc;
use url::Url;


#[derive(Clone, Debug)]
pub struct HistoryEntry {
    pub title: String,
    pub url: Url,
    pub timestamp: DateTime<Local>,
    pub visited_count: u16,

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
    pub fn new() -> Result<Self> {
        info!("Creating history object");
        let connection = Arc::new(Connection::open(History::get_history_filename())?);
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
            )?;
        Ok(History {
            stack: Vec::new(),
            sql: connection,
        })
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

    pub fn add(&mut self, entry: HistoryEntry) -> Result<()> {
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
                .prepare("UPDATE history SET visitedcount=visitedcount+1 WHERE url=?1")?;
            stmt.query(params![&entry.url.to_string()])?;
        } else {
            trace!("History::add(): Adding entry");
            self.sql
                .execute(
                    "INSERT INTO history (url) values (?1)",
                    &[&entry.url.to_string()],
                )?;
        }
        Ok(())
    }

    pub fn clear(&mut self) -> Result<()> {
        trace!("History::clear()");
        self.stack.clear();
        self.sql
            .execute("DELETE FROM history", NO_PARAMS)?;
        Ok(())
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

    pub fn get_latest_history(&self, num_items: usize) -> Result<Vec<HistoryEntry>> {
        let mut res = Vec::<HistoryEntry>::new();
        let mut stmt = self
            .sql
            .prepare(
                "SELECT title, url, timestmp, visitedcount FROM history ORDER BY timestmp DESC LIMIT ?1",
            )?;
        let mut rows = stmt
            .query(params![num_items as u32])?;
        while let Some(row) = rows.next()? {
            let title: String = row.get(1)?;
            let entry = HistoryEntry {
                title,
                url: row.get(1)?,
                timestamp: row.get(2)?,
                visited_count: row.get(3)?,
                position: 0,
            };
            res.push(entry);
        }
        trace!("Returning {} history entries", res.len());
        Ok(res)
    }
}
