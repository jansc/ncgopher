use config::{Config, File, FileFormat};
use serde::{Serialize, Serializer};
use std::fs::File as FsFile;
use std::io::Write;
use std::path::Path;
use url::Url;

fn url_serialize<S>(url: &Url, s: S) -> Result<S::Ok, S::Error>
where
    S: Serializer,
{
    s.serialize_str(url.clone().into_string().as_str())
}

#[derive(Clone, Debug, serde::Serialize)]
pub struct Bookmark {
    pub title: String,
    #[serde(serialize_with = "url_serialize")]
    pub url: Url,
    pub tags: Vec<String>,
}

#[derive(Clone, Debug, Serialize)]
pub struct Bookmarks {
    /// All bookmarks
    pub entries: Vec<Bookmark>,
}

impl Bookmarks {
    pub fn new() -> Bookmarks {
        let mut s = Config::new();
        let confdir = Bookmarks::get_bookmark_filename();
        println!("Looking for bookmarks file {}", confdir);
        if Path::new(confdir.as_str()).exists() {
            match s.merge(File::new(confdir.as_str(), FileFormat::Toml)) {
                Ok(_s) => (),
                Err(e) => {
                    println!("Could not read bookmarks file: {}", e);
                }
            }
        }
        let mut entries = Vec::new();
        println!("Reading bookmarks...");
        info!("bookmarks: {:?}", s.get_array("bookmark"));
        if let Ok(e) = s.get_array("bookmark") {
            for value in e {
                if let Ok(v) = value.into_table() {
                    if let Ok(u) = Url::parse(v["url"].clone().into_str().unwrap().as_str()) {
                        let h = Bookmark {
                            url: u.clone(),
                            title: v["title"].clone().into_str().unwrap(),
                            tags: Vec::<String>::new(),
                        };
                        entries.push(h.clone());
                    }
                }
            }
        }
        Bookmarks { entries }
    }

    fn get_bookmark_filename() -> String {
        let confdir: String = match dirs::config_dir() {
            Some(mut dir) => {
                dir.push(env!("CARGO_PKG_NAME"));
                dir.push("bookmarks");
                dir.into_os_string().into_string().unwrap()
            }
            None => String::new(),
        };
        info!("Looking for bookmark file {}", confdir);
        confdir
    }

    // Checks if a bookmark with a given url exists
    pub fn exists(&self, url: Url) -> bool {
        self.entries.iter().any(|v| v.url == url)
    }

    pub fn add(&mut self, entry: Bookmark) {
        info!("Adding entry to bookmark: {:?}", entry);
        self.entries.push(entry);
        match self.write_bookmarks_to_file() {
            Err(why) => warn!("Could not write bookmarks file: {}", why),
            Ok(()) => (),
        }
    }

    pub fn remove(&mut self, u: Url) {
        info!("Removing entry to bookmark: {:?}", u);
        self.entries.retain(|e| e.url != u);
        match self.write_bookmarks_to_file() {
            Err(why) => warn!("Could not write bookmarks file: {}", why),
            Ok(()) => (),
        }
    }

    pub fn get_bookmarks(&self) -> Vec<Bookmark> {
        let mut res = Vec::<Bookmark>::new();
        for i in 0..self.entries.len() {
            res.push(self.entries[i].clone());
        }
        res
    }

    pub fn write_bookmarks_to_file(&mut self) -> std::io::Result<()> {
        let filename = Bookmarks::get_bookmark_filename();
        info!("Saving bookmarks to file: {}", filename);
        // Create a path to the desired file
        let path = Path::new(&filename);

        let mut file = match FsFile::create(&path) {
            Err(why) => return Err(why),
            Ok(file) => file,
        };

        if let Err(why) = file.write(b"# Automatically generated by ncgopher.\n") {
            return Err(why);
        };
        for b in self.clone().entries {
            if let Err(why) = file.write(b"\n[[bookmark]]\n") {
                return Err(why);
            };
            let item = toml::to_string(&b).unwrap();
            if let Err(why) = file.write_all(item.as_bytes()) {
                return Err(why);
            };
        }
        Ok(())
    }
}
