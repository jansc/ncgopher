use std::sync::mpsc;
use std::sync::{Mutex, RwLock, Arc};
use std::thread;
use std::net::{TcpStream, ToSocketAddrs};
use std::io::{BufWriter, Read, Write};
use std::fs::File;
use std::path::Path;
use cursive::Cursive;
use chrono::Local;
use url::Url;
//use std::process::exit;

use crate::ncgopher::{NcGopher, UiMessage, ContentType};
use crate::gophermap::{GopherMapEntry};
use crate::history::{History, HistoryEntry};
use crate::bookmarks::{Bookmark, Bookmarks};

#[derive(Clone)]
pub struct Controller {
    /// Message channel for communication with the UI
    rx: Arc<mpsc::Receiver<ControllerMessage>>,
    tx: Arc<RwLock<mpsc::Sender<ControllerMessage>>>,
    /// The UI
    ui: Arc<RwLock<NcGopher>>,
    /// The browsing history
    history: Arc<Mutex<History>>,
    /// Bookmarks
    bookmarks: Arc<Mutex<Bookmarks>>,
    /// Current textual content
    content: Arc<Mutex<String>>, 
    /// Current URL
    current_url: Arc<Mutex<Url>>, 
    /// Current content type
    current_content_type: Arc<Mutex<ContentType>>, 
}

/// Defines messages sent between Controller and UI
pub enum ControllerMessage {
    AddBookmark(Url, String, String),
    AddToHistory(Url),
    BinaryWritten(String, usize),
    ClearHistory,
    NavigateBack,
    ReloadCurrentPage,
    RequestAddBookmarkDialog,
    RequestSaveAsDialog,
    SavePageAs(String),
    SetContent(Url, String, ContentType),
    ShowMessage(String),
    RedrawHistory,
    FetchUrl(Url, ContentType, String),
    FetchBinaryUrl(Url, String),
}

impl Controller {

    /// Create a new controller
    pub fn new(app: Cursive, url: Url) -> Result<Controller, String> {
        let (tx, rx) = mpsc::channel::<ControllerMessage>();
        let mut ncgopher = NcGopher::new(app, tx.clone());

        let controller = Controller {
            rx: Arc::new(rx),
            tx: Arc::new(RwLock::new(tx)),
            ui: Arc::new(RwLock::new(ncgopher.clone())),
            history: Arc::new(Mutex::new(History::new())),
            bookmarks: Arc::new(Mutex::new(Bookmarks::new())),
            content: Arc::new(Mutex::new(String::new())),
            current_url: Arc::new(Mutex::new(Url::parse("gopher://host.none").unwrap())),
            current_content_type: Arc::new(Mutex::new(ContentType::Gophermap)),
        };
        ncgopher.setup_ui();
        // Add old entries to history on start-up
        let entries = controller.history.lock().unwrap().get_latest_history(10);
        for entry in entries {
            controller.ui.read().unwrap().ui_tx.read().unwrap()
                .send(UiMessage::AddToHistoryMenu(entry))
                .unwrap();
        }
        info!("Adding existing bookmarks to menu");
        let entries = controller.bookmarks.lock().unwrap().get_bookmarks();
        for entry in entries {
            info!("Found bookmark entry");
            controller.ui.read().unwrap().ui_tx.read().unwrap()
                .send(UiMessage::AddToBookmarkMenu(entry))
                .unwrap();
        }
        // Add bookmarks to bookmark menu on startup
        ncgopher.open_gopher_url(url);
        info!("Controller::new()");
        Ok(controller.clone())
    }

    fn fetch_url(&self, url: Url, content_type: ContentType, query: String) {
        info!("Controller::fetch_url({})", url);
        let tx_clone = self.tx.read().unwrap().clone();

        // Local copy of Url will be passed to thread
        let gopher_url = url.clone();

        let mut port: u16 = 70;
        let p = gopher_url.port();
        match p {
            Some(p) => port = p,
            _ => ()
        }
        let s = gopher_url.host();
        let mut server: String = "host.error".to_string();
        match s {
            Some(s) => server = s.to_string(),
            _ => ()
        }
        let path = gopher_url.path().to_string();
        
        let server_details = format!("{}:{}", server, port).to_string();
        let _server: Vec<_>;
        match server_details.as_str().to_socket_addrs() {
            Ok(s) => { _server = s.collect(); },
            Err(e) => {
                tx_clone.send(
                    ControllerMessage::ShowMessage(format!("Unable to resolve domain: {}", e)))
                    .unwrap();
                return;
            }
        }

        thread::spawn(move || {
            // FIXME: Should use _server instead?
            let mut stream = TcpStream::connect(server_details).expect("Couldn't connect to the server...");
            // FIXME: Error handling!
            if !query.is_empty() {
                write!(stream, "{}\t{}\n", path, query.as_str()).unwrap();
            } else {
                write!(stream, "{}\n", path).unwrap();
            }

            let mut buf = vec![];
            loop {
                match stream.read_to_end(&mut buf) {
                    Ok(_) => break,
                    Err(e) => panic!("encountered IO error: {}", e),
                };
            }
            let s = String::from_utf8_lossy(&buf);
            tx_clone.send(
                ControllerMessage::AddToHistory(url.clone()))
                .unwrap();
            tx_clone.send(
                ControllerMessage::RedrawHistory).unwrap();
            tx_clone.send(
                ControllerMessage::SetContent(gopher_url.clone(), s.to_string(), content_type))
                .unwrap();
        });
    }


    fn fetch_binary_url(&self, url: Url, local_filename: String) {
        let tx_clone = self.tx.read().unwrap().clone();
        // Local copy of Url will be passed to thread
        let gopher_url = url.clone();

        let mut port: u16 = 70;
        let p = gopher_url.port();
        match p {
            Some(p) => port = p,
            _ => ()
        }
        let s = gopher_url.host();
        let mut server: String = "host.error".to_string();
        match s {
            Some(s) => server = s.to_string(),
            _ => ()
        }
        let path = gopher_url.path().to_string();
        
        let server_details = format!("{}:{}", server, port).to_string();
        let _server: Vec<_>;
        match server_details.as_str().to_socket_addrs() {
            Ok(s) => { _server = s.collect(); },
            Err(e) => {
                tx_clone.send(
                    ControllerMessage::ShowMessage(format!("Unable to resolve domain: {}", e)))
                    .unwrap();
                return;
            }
        }

        thread::spawn(move || {
            // FIXME: Error handling!
            let f = File::create(local_filename.clone())
                .expect(format!("Unable to open file '{}'", local_filename.clone()).as_str());
            let mut bw = BufWriter::new(f);
            let mut stream = TcpStream::connect(server_details.clone())
                .expect(format!("Couldn't connect to the server {}", server_details).as_str());

            write!(stream, "{}\n", path).unwrap();

            //let mut buf = Vec::<u8>::new();
            let mut buf = [0u8;1024];
            let mut total_written: usize = 0;
            loop {
                let bytes_read = stream.read(&mut buf).expect("Could not read from TCP");
                if bytes_read == 0 {
                    break;
                }
                let bytes_written = bw.write(&buf[..bytes_read]).expect("Could not write to file");
                total_written += bytes_written;
                tx_clone.send(
                    ControllerMessage::ShowMessage(format!("{} bytes read", total_written)))
                    .unwrap();
            }
            tx_clone.send(
                ControllerMessage::BinaryWritten(local_filename.clone(), total_written))
                .unwrap();
        });
    }

    // TODO: Move from ncgopher.rs:
    // fetch_binary_file
    // open_gopher_url
    // open_gopher_address

    fn add_bookmark(&mut self, url: Url, title: String, tags: String) -> Bookmark {
        let tags: Vec<String> = tags.as_str().split_whitespace().map(String::from).collect();
        let b: Bookmark = Bookmark {
            title: title,
            url: url.clone(),
            tags: tags
        };
        self.bookmarks.lock().unwrap().add(b.clone());
        let toml = toml::to_string(&self.bookmarks.lock().unwrap().clone()).unwrap();
        info!("TOML={}", toml);
        let toml2 = toml::to_string(&b).unwrap();
        info!("TOML2={}", toml2);
        b
    }

    fn add_to_history(&mut self, url: Url) -> HistoryEntry {
        let h: HistoryEntry = HistoryEntry {
            title: url.clone().into_string(),
            url: url.clone(),
            timestamp: Local::now(),
            visited_count: 1,
        };
        self.history.lock().unwrap().add(h.clone());
        h
    }


    /// Purges the entire history
    /// TODO: Add option to clear only parts of the history
    fn clear_history(&mut self) {
        // Purge file
        self.history.lock().unwrap().clear();
    }

    /// Navigates to the previous page in history
    fn navigate_back(&mut self) {
        let mut guard = self.history.lock().unwrap();
        let history = guard.back();
        if history.is_some() {
            let h = history.unwrap();
            std::mem::drop(guard);
            // FIXME: Add contenttype to history
            self.ui.read().unwrap().ui_tx.read().unwrap()
                .send(UiMessage::OpenUrl(h.url, ContentType::Gophermap))
                .unwrap();
        } else {
            std::mem::drop(guard);
            //self.app.add_layer(Dialog::info("No url"))
        }
    }

    fn save_textfile(&mut self, filename: String) {
        let content: String;
        {
            let guard = self.content.lock().unwrap();
            content = guard.clone();
        }
        info!("Save textfile: {}", filename);
        // Create a path to the desired file
        let path = Path::new(filename.as_str());
        let display = path.display();

        let mut file = match File::create(&path) {
            Err(why) => panic!("couldn't open {}: {}", display, why),
            Ok(file) => file,
        };

        // Read the file contents into a string, returns `io::Result<usize>`
        match file.write_all(content.as_bytes()) {
            Err(why) => panic!("couldn't write {}: {}", display, why),
            Ok(_) => (),
        }
        // `file` goes out of scope, and the [filename] file gets closed
    }

    fn save_gophermap(&mut self, filename: String) {
        let content: String;
        {
            let guard = self.content.lock().unwrap();
            content = guard.clone();
        }

        let lines = content.lines();
        let mut txtlines = Vec::<String>::new();
        let mut first = true;
        for l in lines {
            if first {
                first = false;
                continue;
            }
            if l != "." {
                let gophermap_line = GopherMapEntry::parse(l.to_string());
                txtlines.push(gophermap_line.label().to_string());
            }
        }
        info!("Save textfile: {}", filename);
        // Create a path to the desired file
        let path = Path::new(filename.as_str());
        let display = path.display();

        let mut file = match File::create(&path) {
            Err(why) => panic!("couldn't open {}: {}", display, why),
            Ok(file) => file,
        };

        // Read the file contents into a string, returns `io::Result<usize>`
        for l in txtlines {
            match file.write_all(format!("{}\n",l).as_bytes()) {
                Err(why) => panic!("couldn't write {}: {}", display, why),
                Ok(_) => (),
            }
        }
        // `file` goes out of scope, and the "hello.txt" file gets closed
    }

    /// Run the controller
    pub fn run(&mut self) {
        while self.ui.write().unwrap().step() {
            while let Some(message) = self.rx.try_iter().next() {
                // Handle messages arriving from the UI.
                match message {
                    ControllerMessage::AddBookmark(url, tittel, tags) => {
                        let b = self.add_bookmark(url, tittel, tags);
                        self.ui.read().unwrap().ui_tx.read().unwrap()
                            .send(UiMessage::AddToBookmarkMenu(b)).unwrap();
                    },
                    ControllerMessage::AddToHistory(url) => {
                        let h = self.add_to_history(url);
                        self.ui.read().unwrap().ui_tx.read().unwrap()
                            .send(UiMessage::AddToHistoryMenu(h)).unwrap();
                    },
                    ControllerMessage::ClearHistory => {
                        self.clear_history();
                        self.ui.read().unwrap().ui_tx.read().unwrap()
                            .send(UiMessage::ClearHistoryMenu).unwrap();
                    },
                    ControllerMessage::ReloadCurrentPage => {
                        let current_url: Url;
                        let current_content_type: ContentType;
                        {
                            let guard = self.current_url.lock().unwrap();
                            current_url = guard.clone();
                            let guard = self.current_content_type.lock().unwrap();
                            current_content_type = guard.clone();
                        }
                        self.ui.read().unwrap().ui_tx.read().unwrap()
                            .send(UiMessage::OpenUrl(current_url, current_content_type)).unwrap();
                    },
                    ControllerMessage::RequestAddBookmarkDialog => {
                        let current_url: Url;
                        {
                            let guard = self.current_url.lock().unwrap();
                            current_url = guard.clone();
                        }
                        self.ui.read().unwrap().ui_tx.read().unwrap()
                            .send(UiMessage::ShowAddBookmarkDialog(current_url)).unwrap();
                    },
                    ControllerMessage::RequestSaveAsDialog => {
                        let current_url: Url;
                        {
                            let guard = self.current_url.lock().unwrap();
                            current_url = guard.clone();
                        }
                        self.ui.read().unwrap().ui_tx.read().unwrap()
                            .send(UiMessage::ShowSaveAsDialog(current_url)).unwrap();
                    },
                    ControllerMessage::SavePageAs(filename) => {
                        let url: Url;
                        let content_type: ContentType;
                        {
                            let guard = self.current_content_type.lock().unwrap();
                            content_type = guard.clone();
                            let guard = self.current_url.lock().unwrap();
                            url = guard.clone();
                        }
                        match content_type {
                            ContentType::Gophermap => { self.save_gophermap(filename.clone()) },
                            ContentType::Text => { self.save_textfile(filename.clone()) },
                            _ => ()
                        }
                        self.ui.read().unwrap().ui_tx.read().unwrap()
                            .send(UiMessage::PageSaved(url, content_type, filename)).unwrap();
                    },
                    ControllerMessage::SetContent(url, content, content_type) => {
                        {
                            let mut guard = self.content.lock().unwrap();
                            guard.clear();
                            guard.push_str(content.as_str());
                            let mut guard = self.current_url.lock().unwrap();
                            *guard = url.clone();
                            let mut guard = self.current_content_type.lock().unwrap();
                            *guard = content_type.clone();
                        }
                        self.ui.read().unwrap().ui_tx.read().unwrap()
                            .send(UiMessage::ShowContent(url, content, content_type)).unwrap();
                    },
                    ControllerMessage::ShowMessage(msg) => {
                        self.ui.read().unwrap().ui_tx.read().unwrap()
                            .send(UiMessage::ShowMessage(msg)).unwrap();
                    },
                    ControllerMessage::BinaryWritten(filename, bytes_written) => {
                        self.ui.read().unwrap().ui_tx.read().unwrap()
                            .send(UiMessage::BinaryWritten(filename, bytes_written)).unwrap();
                    },
                    ControllerMessage::NavigateBack => {
                        self.navigate_back();
                    },
                    ControllerMessage::FetchUrl(url, content_type, query) => {
                        self.fetch_url(url, content_type, query);
                    },
                    ControllerMessage::FetchBinaryUrl(url, local_path) => {
                        self.fetch_binary_url(url, local_path);
                    },
                    ControllerMessage::RedrawHistory => {
                        info!("Controller: Clearing history");
                        self.ui.read().unwrap().ui_tx.read().unwrap()
                            .send(UiMessage::ClearHistoryMenu).unwrap();
                        let entries = self.history.lock().unwrap().get_latest_history(10);
                        for entry in entries {
                            info!("Controller: readding entry {:?}", entry);
                            self.ui.read().unwrap().ui_tx.read().unwrap()
                                .send(UiMessage::AddToHistoryMenu(entry))
                                .unwrap();
                        }
                    }
                };
            }
        }
    }
}
