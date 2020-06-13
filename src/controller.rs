use chrono::Local;
use chrono::{Duration, Utc};
use cursive::Cursive;
use lazy_static::lazy_static;
use regex::Regex;
use std::error::Error;
use std::fs::File;
use std::io::{BufRead, BufReader, BufWriter, Read, Write};
use std::net::{TcpStream, ToSocketAddrs};
use std::path::Path;
use std::process::Command;
use std::sync::mpsc;
use std::sync::{Arc, Mutex, RwLock};
use std::thread;
use url::Url;

use crate::bookmarks::{Bookmark, Bookmarks};
use crate::gemini::GeminiType;
use crate::gophermap::{GopherMapEntry, ItemType};
use crate::history::{History, HistoryEntry};
use crate::ncgopher::{NcGopher, UiMessage};
use crate::SETTINGS;
#[cfg(feature = "tls")]
use native_tls::{Protocol, TlsConnector};
use x509_parser::parse_x509_der;

lazy_static! {
    static ref LAST_REQUEST_ID: Mutex<i64> =  Mutex::new(0);
}

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
}

/// Defines messages sent between Controller and UI
pub enum ControllerMessage {
    AddBookmark(Url, String, String),
    AddToHistory(Url),
    BinaryWritten(String, usize),
    ClearHistory,
    NavigateBack,
    OpenImage(Url),
    OpenHtml(Url),
    OpenTelnet(Url),
    ReloadCurrentPage,
    RemoveBookmark(Bookmark),
    RequestAddBookmarkDialog,
    RequestEditBookmarksDialog,
    RequestGeminiQueryDialog(Url, String),
    RequestSaveAsDialog,
    RequestSettingsDialog,
    SavePageAs(String),
    SetContent(Url, String, ItemType, usize),
    SetGeminiContent(Url, GeminiType, String),
    ShowMessage(String),
    RedrawBookmarks,
    RedrawHistory,
    FetchGeminiUrl(Url, bool, usize),
    FetchUrl(Url, ItemType, bool, usize),
    FetchBinaryUrl(Url, String),
}

impl Drop for Controller {
    fn drop(&mut self) {
        // Cleanup
    }
}

impl Controller {
    /// Create a new controller
    pub fn new(app: Cursive, url: Url) -> Result<Controller, Box<dyn Error>> {
        let (tx, rx) = mpsc::channel::<ControllerMessage>();
        let mut ncgopher = NcGopher::new(app, tx.clone());

        let controller = Controller {
            rx: Arc::new(rx),
            tx: Arc::new(RwLock::new(tx)),
            ui: Arc::new(RwLock::new(ncgopher.clone())),
            history: Arc::new(Mutex::new(History::new()?)),
            bookmarks: Arc::new(Mutex::new(Bookmarks::new())),
            content: Arc::new(Mutex::new(String::new())),
            current_url: Arc::new(Mutex::new(Url::parse("gopher://host.none").unwrap())),
        };
        ncgopher.setup_ui();
        // Add old entries to history on start-up
        let entries = controller
            .history
            .lock()
            .unwrap()
            .get_latest_history(10)
            .expect("Could not get latest history");
        for entry in entries {
            controller
                .ui
                .read()
                .unwrap()
                .ui_tx
                .read()
                .unwrap()
                .send(UiMessage::AddToHistoryMenu(entry))?;
        }
        info!("Adding existing bookmarks to menu");
        let entries = controller.bookmarks.lock().unwrap().get_bookmarks();
        for entry in entries {
            info!("Found bookmark entry");
            controller
                .ui
                .read()
                .unwrap()
                .ui_tx
                .read()
                .unwrap()
                .send(UiMessage::AddToBookmarkMenu(entry))?;
        }
        // Add bookmarks to bookmark menu on startup
        ncgopher.open_url(url, true, 0);
        info!("Controller::new()");
        Ok(controller)
    }

    // Used for gemini downloads
    fn get_filename_from_url(&self, url: &Url) -> String {
        if let Some(mut segments) = url.path_segments().map(|c| c.collect::<Vec<_>>()) {
            let last_seg = segments.pop();
            if let Some(filename) = last_seg {
                return filename.to_string();
            }
        }
        // TODO: Create extension based on mimetype
        "download.bin".to_string()
    }

    fn fetch_gemini_url(&self, url: Url, add_to_history: bool, _index: usize) {
        trace!("Controller::fetch_gemini_url({})", url);
        let tx_clone = self.tx.read().unwrap().clone();

        let request_id: i64;
        {
            let mut guard = LAST_REQUEST_ID.lock().unwrap();
            *guard += 1;
            request_id = *guard;
        }

        // Local copy of Url will be passed to thread
        let gemini_url = url.clone();

        let mut port: u16 = 1965;
        let p = gemini_url.port();
        if let Some(p) = p {
            port = p
        };
        let s = gemini_url.host();
        let mut server: String = "host.error".to_string();
        if let Some(s) = s {
            server = s.to_string()
        }
        info!("fetch_gemini_url(): About to open gemini URL {}", url);

        let server_details = format!("{}:{}", server, port);
        let _server: Vec<_>;
        match server_details.as_str().to_socket_addrs() {
            Ok(s) => {
                _server = s.collect();
            }
            Err(e) => {
                tx_clone
                    .send(ControllerMessage::ShowMessage(format!(
                        "Unable to resolve domain: {}",
                        e
                    )))
                    .unwrap();
                return;
            }
        }
        let local_filename = self.get_filename_from_url(&url);
        thread::spawn(move || {
            // FIXME: Should use _server instead?
            let mut buf = String::new();
            let mut builder = TlsConnector::builder();
            // Self-signed certificates are considered invalid, but they are quite
            // common for gemini servers. Therefore, we accept invalid certs,
            // but check for expiration later
            builder.danger_accept_invalid_certs(true);

            // Rust's native-tls does not yet provide Tlsv13 :(
            // Alternative implementation: rusttls
            builder.min_protocol_version(Some(Protocol::Tlsv12));
            match builder.build() {
                Ok(connector) => {
                    let tlsstream;
                    let stream = TcpStream::connect(server_details.clone());
                    match stream {
                        Ok(stream) => tlsstream = stream,
                        Err(err) => {
                            tx_clone
                                .send(ControllerMessage::ShowMessage(format!(
                                    "Could not connect to server: {}",
                                    err
                                )))
                                .unwrap();
                            return;
                        }
                    }
                    match connector.connect(&server, tlsstream) {
                        Ok(mut stream) => {
                            info!("Connected with TLS");

                            // get peer certificate
                            match stream.peer_certificate() {
                                Err(err) => {
                                    tx_clone
                                        .send(ControllerMessage::ShowMessage(format!(
                                            "Could not get perr certificate: {:?}",
                                            err
                                        )))
                                        .unwrap();
                                    return;
                                }
                                Ok(option) => {
                                    if let Some(cert) = option {
                                        info!(
                                            "Peer certificate: {:?}",
                                            base64::encode(cert.to_der().unwrap())
                                        );
                                        match parse_x509_der(&cert.to_der().unwrap()) {
                                            Ok((_rem, cert)) => {
                                                info!("Successfully parsed certificate");
                                                match cert
                                                    .tbs_certificate
                                                    .validity
                                                    .time_to_expiration()
                                                {
                                                    Some(duration) => {
                                                        let now = Utc::now();
                                                        let expires = now.checked_add_signed(
                                                            Duration::from_std(duration).unwrap(),
                                                        );
                                                        match expires {
                                                            Some(x) => info!("Certificate expires {}", x.to_rfc3339()),
                                                            None => warn!("Certificate expire date overflows!"),
                                                        }

                                                        info!("Certificate valid {:?}", duration);
                                                    }
                                                    None => {
                                                        tx_clone
                                                            .send(ControllerMessage::ShowMessage(
                                                                "Server certificate expired."
                                                                    .to_string(),
                                                            ))
                                                            .unwrap();
                                                    }
                                                }
                                            }
                                            Err(err) => {
                                                tx_clone
                                                    .send(ControllerMessage::ShowMessage(format!(
                                                        "Could not parse peer certificate: {:?}",
                                                        err
                                                    )))
                                                    .unwrap();
                                            }
                                        }
                                        // Store certificate if not known
                                    }
                                }
                            }
                            info!("Writing url '{}'", url.as_str());
                            write!(stream, "{}\r\n", url.as_str()).unwrap();
                            let mut bufr = BufReader::new(stream);
                            info!("Reading from gemini stream");
                            // Read Gemini Header
                            match bufr.read_line(&mut buf) {
                                Ok(_) => (),
                                Err(e) => {
                                    tx_clone
                                        .send(ControllerMessage::ShowMessage(format!(
                                            "I/O error: {}",
                                            e
                                        )))
                                        .unwrap();
                                    return;
                                }
                            };
                            let buf = buf.trim();
                            // "text/gemini; charset=utf-8"
                            info!("Got gemini header: {}:  {}", buf.len(), buf);

                            {
                                let guard = LAST_REQUEST_ID.lock().unwrap();
                                if request_id < *guard {
                                    return;
                                }
                            }

                            // TODO: Check status code
                            // if status[0] == '2'
                            // check mime-type:
                            //  - text/gemini => show as gemini content
                            //  - text/xxx    => show as text content
                            //  - otherwise   => download as binary
                            // status[0] == '3' => redirect
                            // status[0] == '4' => show specific error message
                            // status[0] == '5' => show specific error message
                            // status[0] == '6' => invalid certificate, show specific error message
                            if buf.is_empty() {
                                tx_clone
                                    .send(ControllerMessage::ShowMessage(
                                        "Could not read from stream".to_string(),
                                    ))
                                    .unwrap();
                                return;
                            }
                            match buf.chars().next().unwrap() {
                                '1' => {
                                    let status1 = Regex::new(r"^10\s+(.*)$").unwrap();
                                    if let Some(caps) = status1.captures(&buf) {
                                        let query = caps.get(1).unwrap().as_str();
                                        info!("Got query: {}", query);
                                        tx_clone
                                            .send(ControllerMessage::RequestGeminiQueryDialog(
                                                url,
                                                query.to_string(),
                                            ))
                                            .unwrap();
                                    }
                                    // TODO: Handle password inputs
                                    return;
                                }
                                '2' => {
                                    let status2 = Regex::new(r"^(2[01])\s+(.*);?").unwrap();
                                    if let Some(caps) = status2.captures(&buf) {
                                        let mimetype = caps.get(2).unwrap().as_str();
                                        // If mimetype is text/* download as gemini
                                        // Otherwise initiate a binary download
                                        if mimetype.starts_with("text/") {
                                            let mut buf = vec![];
                                            bufr.read_to_end(&mut buf).unwrap_or_else(|err| {
                                                tx_clone
                                                    .send(ControllerMessage::ShowMessage(format!(
                                                        "I/O error: {}",
                                                        err
                                                    )))
                                                    .unwrap();
                                                0
                                            });
                                            let s = String::from_utf8_lossy(&buf);
                                            tx_clone
                                                .send(ControllerMessage::SetGeminiContent(
                                                    gemini_url.clone(),
                                                    GeminiType::Gemini,
                                                    s.to_string(),
                                                ))
                                                .unwrap();
                                            if add_to_history {
                                                tx_clone
                                                    .send(ControllerMessage::AddToHistory(
                                                        gemini_url.clone(),
                                                    ))
                                                    .unwrap();
                                            }
                                            tx_clone
                                                .send(ControllerMessage::RedrawHistory)
                                                .unwrap();
                                        } else {
                                            // Binary download
                                            let f = File::create(local_filename.clone())
                                                .unwrap_or_else(|_| {
                                                    panic!(
                                                        "Unable to open file '{}'",
                                                        local_filename.clone()
                                                    )
                                                });
                                            let mut bw = BufWriter::new(f);
                                            let mut buf = [0u8; 1024];
                                            let mut total_written: usize = 0;
                                            loop {
                                                let bytes_read = bufr
                                                    .read(&mut buf)
                                                    .expect("Could not read from TCP");
                                                if bytes_read == 0 {
                                                    break;
                                                }
                                                let bytes_written = bw
                                                    .write(&buf[..bytes_read])
                                                    .expect("Could not write to file");
                                                total_written += bytes_written;
                                                tx_clone
                                                    .send(ControllerMessage::ShowMessage(format!(
                                                        "{} bytes read",
                                                        total_written
                                                    )))
                                                    .unwrap();
                                            }
                                            tx_clone
                                                .send(ControllerMessage::BinaryWritten(
                                                    local_filename.clone(),
                                                    total_written,
                                                ))
                                                .unwrap();
                                        }
                                    } else {
                                        tx_clone
                                            .send(ControllerMessage::ShowMessage(format!(
                                                "Invalid status code: {}",
                                                buf
                                            )))
                                            .unwrap();
                                    }
                                }
                                '3' => {
                                    let status3 = Regex::new(r"^(3[01])\s+(.*)\s*$?").unwrap();
                                    if let Some(caps) = status3.captures(&buf) {
                                        // TODO: Should automatically update bookmarks when code is 31
                                        let _code = caps.get(1).unwrap().as_str();
                                        let url = caps.get(2).unwrap().as_str();
                                        // FIXME: Try to parse url, check scheme
                                        if let Ok(url) = Url::parse(url) {
                                            tx_clone
                                                .send(ControllerMessage::FetchGeminiUrl(
                                                    url, true, 0,
                                                ))
                                                .unwrap();
                                        } else {
                                            tx_clone
                                                .send(ControllerMessage::ShowMessage(format!(
                                                    "Invalid redirect url: {}",
                                                    url
                                                )))
                                                .unwrap();
                                        }
                                    } else {
                                        tx_clone
                                            .send(ControllerMessage::ShowMessage(format!(
                                                "Invalid header from server: {}",
                                                buf
                                            )))
                                            .unwrap();
                                    }
                                    return;
                                }
                                '4' | '5' | '6' => {
                                    let _status4 = Regex::new(r"^(4[01234])\s+(.*)\s+$?").unwrap();
                                    let _status5 = Regex::new(r"^(5[01239])\s+(.*)\s+$?").unwrap();
                                    let _status6 = Regex::new(r"^(6[012345])\s+(.*)\s+$?").unwrap();
                                    tx_clone
                                        .send(ControllerMessage::ShowMessage(format!(
                                            "Gemini error: {}",
                                            buf
                                        )))
                                        .unwrap();
                                    return;
                                }
                                _ => {
                                    tx_clone
                                        .send(ControllerMessage::ShowMessage(format!(
                                            "Unhandled status code: {}",
                                            buf
                                        )))
                                        .unwrap();
                                    return;
                                }
                            }
                            info!("Finished reading from gemini stream");
                        }
                        Err(err) => {
                            warn!("Could not open tls stream: {} to {}", err, server_details);
                            tx_clone
                                .send(ControllerMessage::ShowMessage(format!(
                                    "Could not open tls stream to {}: {}",
                                    server_details, err
                                )))
                                .unwrap();
                        }
                    }
                }
                Err(err) => {
                    tx_clone
                        .send(ControllerMessage::ShowMessage(format!(
                            "Could not establish connection: {}",
                            err
                        )))
                        .unwrap();
                }
            }
        });
    }

    //    fn binary_download(filename: String) {
    //    }

    fn fetch_url(&self, url: Url, item_type: ItemType, add_to_history: bool, index: usize) {
        // index is the position in the text (used when navigatin back or reloading)
        trace!("Controller::fetch_url({})", url);
        let tx_clone = self.tx.read().unwrap().clone();
        let request_id: i64;
        {
            let mut guard = LAST_REQUEST_ID.lock().unwrap();
            *guard += 1;
            request_id = *guard;
        }

        // Local copy of Url will be passed to thread
        let gopher_url = url.clone();

        let mut port: u16 = 70;
        let p = gopher_url.port();
        if let Some(p) = p {
            port = p
        };
        let s = gopher_url.host();
        let mut server: String = "host.error".to_string();
        if let Some(s) = s {
            server = s.to_string()
        }
        let path = gopher_url.path();
        let mut path = str::replace(path, "%09", "\t");
        info!("fetch_url(): About to open URL {}", path);
        if path.len() > 2 {
            //let x = path[0..1].to_string();
            // TODO: Sjekk om x[0] == / og x[1] == itemtype
            path = path[2..].to_string();
        } else {
            path = "".to_string();
        }

        let server_details = format!("{}:{}", server, port);
        let _server: Vec<_>;
        match server_details.as_str().to_socket_addrs() {
            Ok(s) => {
                _server = s.collect();
            }
            Err(e) => {
                tx_clone
                    .send(ControllerMessage::ShowMessage(format!(
                        "Unable to resolve domain: {}",
                        e
                    )))
                    .unwrap();
                return;
            }
        }

        thread::spawn(move || {
            // FIXME: Should use _server instead?
            let mut tls = false;
            let mut buf = vec![];
            if cfg!(feature = "tls") {
                if port != 70 {
                    if let Ok(connector) = TlsConnector::new() {
                        let stream = TcpStream::connect(server_details.clone())
                            .expect("Couldn't connect to the server...");
                        match connector.connect(&server, stream) {
                            Ok(mut stream) => {
                                tls = true;
                                info!("Connected with TLS");
                                writeln!(stream, "{}", path).unwrap();

                                loop {
                                    match stream.read_to_end(&mut buf) {
                                        Ok(_) => break,
                                        Err(e) => {
                                            tx_clone
                                                .send(ControllerMessage::ShowMessage(format!(
                                                    "I/O error: {}",
                                                    e
                                                )))
                                                .unwrap();
                                        }
                                    };
                                }
                            }
                            Err(e) => {
                                warn!("Could not open tls stream: {} to {}", e, server_details);
                            }
                        }
                    } else {
                        info!("Could not establish tls connection");
                    }
                }
            } else {
                info!("TLS not configured");
            }
            if !tls {
                match TcpStream::connect(server_details.clone()) {
                    Ok(mut stream) => {
                        writeln!(stream, "{}", path).unwrap();
                        loop {
                            match stream.read_to_end(&mut buf) {
                                Ok(_) => break,
                                Err(e) => {
                                    tx_clone
                                        .send(ControllerMessage::ShowMessage(format!(
                                            "I/O error: {}",
                                            e
                                        )))
                                        .unwrap();
                                }
                            }
                        }
                    }
                    Err(e) => {
                        tx_clone
                            .send(ControllerMessage::ShowMessage(format!(
                                "Couldn't connect to server: {}",
                                e
                            )))
                            .unwrap();
                        return;
                    }
                };
            }
            {
                let guard = LAST_REQUEST_ID.lock().unwrap();
                if request_id < *guard {
                    return;
                }
            }

            let s = String::from_utf8_lossy(&buf);
            if add_to_history {
                tx_clone
                    .send(ControllerMessage::AddToHistory(url.clone()))
                    .unwrap();
            }
            tx_clone.send(ControllerMessage::RedrawHistory).unwrap();
            tx_clone
                .send(ControllerMessage::SetContent(
                    gopher_url.clone(),
                    s.to_string(),
                    item_type,
                    index,
                ))
                .unwrap();
        });
    }

    fn fetch_binary_url(&self, url: Url, local_filename: String) {
        let tx_clone = self.tx.read().unwrap().clone();

        // Local copy of Url will be passed to thread
        let gopher_url = url;

        let mut port: u16 = 70;
        let p = gopher_url.port();
        if let Some(p) = p {
            port = p
        }
        let s = gopher_url.host();
        let mut server: String = "host.error".to_string();
        if let Some(s) = s {
            server = s.to_string()
        }
        let mut path = gopher_url.path().to_string();
        if path.len() > 2 {
            //let x = path[0..1].to_string();
            // TODO: Sjekk om x[0] == / og x[1] == itemtype
            path = path[2..].to_string();
        } else {
            path = "".to_string();
        }

        let server_details = format!("{}:{}", server, port);
        let _server: Vec<_>;
        match server_details.as_str().to_socket_addrs() {
            Ok(s) => {
                _server = s.collect();
            }
            Err(e) => {
                tx_clone
                    .send(ControllerMessage::ShowMessage(format!(
                        "Unable to resolve domain: {}",
                        e
                    )))
                    .unwrap();
                return;
            }
        }

        thread::spawn(move || {
            // FIXME: Error handling!
            let mut tls = false;
            let f = File::create(local_filename.clone())
                .unwrap_or_else(|_| panic!("Unable to open file '{}'", local_filename.clone()));
            let mut bw = BufWriter::new(f);
            let mut buf = [0u8; 1024];
            let mut total_written: usize = 0;
            if cfg!(feature = "tls") {
                if port != 70 {
                    if let Ok(connector) = TlsConnector::new() {
                        let stream =
                            TcpStream::connect(server_details.clone()).unwrap_or_else(|_| {
                                panic!("Couldn't connect to the server {}", server_details)
                            });
                        match connector.connect(&server, stream) {
                            Ok(mut stream) => {
                                tls = true;
                                info!("Connected with TLS");
                                writeln!(stream, "{}", path).unwrap();
                                loop {
                                    let bytes_read =
                                        stream.read(&mut buf).expect("Could not read from TCP");
                                    if bytes_read == 0 {
                                        break;
                                    }
                                    let bytes_written = bw
                                        .write(&buf[..bytes_read])
                                        .expect("Could not write to file");
                                    total_written += bytes_written;
                                    tx_clone
                                        .send(ControllerMessage::ShowMessage(format!(
                                            "{} bytes read",
                                            total_written
                                        )))
                                        .unwrap();
                                }
                            }
                            Err(e) => {
                                warn!("Could not open tls stream: {} to {}", e, server_details);
                            }
                        }
                    } else {
                        info!("Could not establish tls connection");
                    }
                }
            } else {
                info!("TLS not configured");
            }
            if !tls {
                let mut stream = TcpStream::connect(server_details.clone())
                    .expect("Couldn't connect to the server...");
                writeln!(stream, "{}", path).unwrap();
                loop {
                    let bytes_read = stream.read(&mut buf).expect("Could not read from TCP");
                    if bytes_read == 0 {
                        break;
                    }
                    let bytes_written = bw
                        .write(&buf[..bytes_read])
                        .expect("Could not write to file");
                    total_written += bytes_written;
                    tx_clone
                        .send(ControllerMessage::ShowMessage(format!(
                            "{} bytes read",
                            total_written
                        )))
                        .unwrap();
                }
            }
            tx_clone
                .send(ControllerMessage::BinaryWritten(
                    local_filename.clone(),
                    total_written,
                ))
                .unwrap();
        });
    }

    // TODO: Move from ncgopher.rs:
    // fetch_binary_file
    // open_gopher_url
    // open_gopher_address

    fn add_bookmark(&mut self, url: Url, title: String, tags: String) -> Bookmark {
        let tags: Vec<String> = tags.as_str().split_whitespace().map(String::from).collect();
        let b: Bookmark = Bookmark { title, url, tags };
        self.bookmarks.lock().unwrap().add(b.clone());
        b
    }

    fn remove_bookmark(&mut self, b: Bookmark) {
        self.bookmarks.lock().unwrap().remove(b.url);
        let tx_clone = self.tx.read().unwrap().clone();
        tx_clone.send(ControllerMessage::RedrawBookmarks).unwrap();
    }

    fn add_to_history(&mut self, url: Url) -> HistoryEntry {
        let ui = self.ui.read().unwrap();
        if let Some(i) = ui.get_selected_item_index() {
            // Updates the position of the last item on the stack This
            // works because add_to_history is called _before_
            // set_content.
            info!("add_to_history(): updating last item's position to {}", i);
            let mut guard = self.history.lock().unwrap();
            guard.update_selected_item(i);
        }
        info!("add_to_history(): {}", url);
        let h: HistoryEntry = HistoryEntry {
            title: url.clone().into_string(),
            url,
            timestamp: Local::now(),
            visited_count: 1,
            position: 0,
        };
        self.history
            .lock()
            .unwrap()
            .add(h.clone())
            .expect("Could not add to history");
        h
    }

    /// Purges the entire history
    /// TODO: Add option to clear only parts of the history
    fn clear_history(&mut self) {
        // Purge file
        self.history
            .lock()
            .unwrap()
            .clear()
            .expect("Could not clear history");
    }

    /// Navigates to the previous page in history
    fn navigate_back(&mut self) {
        let mut guard = self.history.lock().unwrap();
        let history = guard.back();
        if let Some(h) = history {
            std::mem::drop(guard);
            info!("NAVIGATE_BACK to index {}", h.position);
            self.ui
                .read()
                .unwrap()
                .ui_tx
                .read()
                .unwrap()
                .send(UiMessage::OpenUrl(h.url, false, h.position))
                .unwrap();
        } else {
            std::mem::drop(guard);
            //self.app.add_layer(Dialog::info("No url"))
        }
    }

    fn open_command(&mut self, command: &str, url: Url) -> Result<(), Box<dyn Error>> {
        // Opens an image in an external application - if defined in settings
        let tx_clone = self.tx.read().unwrap().clone();
        let u = url.clone().into_string();
        let command = SETTINGS.read().unwrap().get_str(command)?;
        if !command.is_empty() {
            if let Err(err) = Command::new(&command).arg(u).spawn() {
                tx_clone.send(ControllerMessage::ShowMessage(format!(
                    "Command failed: {}: {}",
                    err, command
                )))?;
            }
        } else {
            tx_clone.send(ControllerMessage::ShowMessage(format!(
                "No command for opening {} defined.",
                url.into_string()
            )))?;
        }
        Ok(())
    }

    /// Saves the current text file to disk
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
            Err(why) => {
                self.ui
                    .read()
                    .unwrap()
                    .controller_tx
                    .read()
                    .unwrap()
                    .send(ControllerMessage::ShowMessage(format!(
                        "Couldn't open {}: {}",
                        display, why
                    )))
                    .unwrap();
                return;
            }
            Ok(file) => file,
        };

        // Read the file contents into a string, returns `io::Result<usize>`
        if let Err(why) = file.write_all(content.as_bytes()) {
            self.ui
                .read()
                .unwrap()
                .controller_tx
                .read()
                .unwrap()
                .send(ControllerMessage::ShowMessage(format!(
                    "Couldn't open {}: {}",
                    display, why
                )))
                .unwrap();
        }
        // `file` goes out of scope, and the [filename] file gets closed
    }

    fn save_gemini(&mut self, _filename: String) {
        // FIXME implement
        warn!("save_gemini(): NOT IMPLEMENTED");
    }

    /// Save the current gophermap to disk
    fn save_gophermap(&mut self, filename: String) {
        let content: String;
        {
            let guard = self.content.lock().unwrap();
            content = guard.clone();
        }
        let tx_clone = self.tx.read().unwrap().clone();
        let lines = content.lines();
        let mut txtlines = Vec::<String>::new();
        let mut first = true;
        for l in lines {
            if first {
                first = false;
                continue;
            }
            if l != "." {
                match GopherMapEntry::parse(l.to_string()) {
                    Ok(gl) => txtlines.push(gl.label().to_string()),
                    Err(err) => {
                        warn!("Invalid gophermap line: {}", err);
                    }
                };
            }
        }
        info!("Save textfile: {}", filename);
        // Create a path to the desired file
        let path = Path::new(filename.as_str());
        let display = path.display();

        let mut file = match File::create(&path) {
            Err(err) => {
                tx_clone
                    .send(ControllerMessage::ShowMessage(format!(
                        "Could open: {}: {}",
                        display, err
                    )))
                    .unwrap();
                return;
            }
            Ok(file) => file,
        };

        // Read the file contents into a string, returns `io::Result<usize>`
        for l in txtlines {
            if let Err(err) = file.write_all(format!("{}\n", l).as_bytes()) {
                tx_clone
                    .send(ControllerMessage::ShowMessage(format!(
                        "Could not write: {}: {}",
                        display, err
                    )))
                    .unwrap();
                return;
            }
        }
        // `file` goes out of scope and the file gets closed
    }

    /// Run the controller
    pub fn run(&mut self) {
        while self.ui.write().unwrap().step() {
            while let Some(message) = self.rx.try_iter().next() {
                // Handle messages arriving from the UI.
                match message {
                    ControllerMessage::AddBookmark(url, tittel, tags) => {
                        let b = self.add_bookmark(url, tittel, tags);
                        self.ui
                            .read()
                            .unwrap()
                            .ui_tx
                            .read()
                            .unwrap()
                            .send(UiMessage::AddToBookmarkMenu(b))
                            .unwrap();
                    }
                    ControllerMessage::AddToHistory(url) => {
                        let h = self.add_to_history(url);
                        self.ui
                            .read()
                            .unwrap()
                            .ui_tx
                            .read()
                            .unwrap()
                            .send(UiMessage::AddToHistoryMenu(h))
                            .unwrap();
                    }
                    ControllerMessage::ClearHistory => {
                        self.clear_history();
                        self.ui
                            .read()
                            .unwrap()
                            .ui_tx
                            .read()
                            .unwrap()
                            .send(UiMessage::ClearHistoryMenu)
                            .unwrap();
                    }
                    ControllerMessage::ReloadCurrentPage => {
                        let current_url: Url;
                        let mut index = 0;
                        {
                            let guard = self.current_url.lock().unwrap();
                            current_url = guard.clone();
                            let ui = self.ui.read().unwrap();
                            if let Some(i) = ui.get_selected_item_index() {
                                index = i;
                            }
                        }
                        self.ui
                            .read()
                            .unwrap()
                            .ui_tx
                            .read()
                            .unwrap()
                            .send(UiMessage::OpenUrl(current_url, false, index))
                            .unwrap();
                    }
                    ControllerMessage::RemoveBookmark(bookmark) => {
                        info!("Removing bookmark {}", bookmark.title);
                        self.remove_bookmark(bookmark);
                    }
                    ControllerMessage::RequestAddBookmarkDialog => {
                        let current_url: Url;
                        {
                            let guard = self.current_url.lock().unwrap();
                            current_url = guard.clone();
                        }
                        self.ui
                            .read()
                            .unwrap()
                            .ui_tx
                            .read()
                            .unwrap()
                            .send(UiMessage::ShowAddBookmarkDialog(current_url))
                            .unwrap();
                    }
                    ControllerMessage::RequestEditBookmarksDialog => {
                        let v: Vec<Bookmark>;
                        {
                            let guard = self.bookmarks.lock().unwrap();
                            v = guard.clone().entries;
                        }
                        self.ui
                            .read()
                            .unwrap()
                            .ui_tx
                            .read()
                            .unwrap()
                            .send(UiMessage::ShowEditBookmarksDialog(v))
                            .unwrap();
                    }
                    ControllerMessage::RequestGeminiQueryDialog(url, query) => {
                        self.ui
                            .read()
                            .unwrap()
                            .ui_tx
                            .read()
                            .unwrap()
                            .send(UiMessage::OpenGeminiQueryDialog(url, query))
                            .unwrap();
                    }
                    ControllerMessage::RequestSaveAsDialog => {
                        let current_url: Url;
                        {
                            let guard = self.current_url.lock().unwrap();
                            current_url = guard.clone();
                        }
                        self.ui
                            .read()
                            .unwrap()
                            .ui_tx
                            .read()
                            .unwrap()
                            .send(UiMessage::ShowSaveAsDialog(current_url))
                            .unwrap();
                    }
                    ControllerMessage::RequestSettingsDialog => {
                        //let settings = self.settings.read().unwrap();
                        self.ui
                            .read()
                            .unwrap()
                            .ui_tx
                            .read()
                            .unwrap()
                            .send(UiMessage::ShowSettingsDialog)
                            .unwrap();
                    }
                    ControllerMessage::SavePageAs(filename) => {
                        let url: Url;
                        {
                            let guard = self.current_url.lock().unwrap();
                            url = guard.clone();
                            warn!("CURRENT URL = {}", url);
                        }

                        match url.scheme() {
                            "gopher" => {
                                let item_type = ItemType::from_url(&url);
                                match item_type {
                                    ItemType::Dir => self.save_gophermap(filename.clone()),
                                    ItemType::File => self.save_textfile(filename.clone()),
                                    _ => (),
                                }
                            }
                            "gemini" => self.save_gemini(filename.clone()),
                            _ => (),
                        }
                        self.ui
                            .read()
                            .unwrap()
                            .ui_tx
                            .read()
                            .unwrap()
                            .send(UiMessage::PageSaved(url, filename))
                            .unwrap();
                    }
                    ControllerMessage::SetGeminiContent(url, gemini_type, content) => {
                        {
                            let mut guard = self.content.lock().unwrap();
                            guard.clear();
                            guard.push_str(content.as_str());
                            warn!("SETGEMINICONTENT {}", url);
                            let mut guard = self.current_url.lock().unwrap();
                            *guard = url.clone();
                        }
                        self.ui
                            .read()
                            .unwrap()
                            .ui_tx
                            .read()
                            .unwrap()
                            .send(UiMessage::ShowGeminiContent(url, gemini_type, content))
                            .unwrap();
                    }
                    ControllerMessage::SetContent(url, content, item_type, index) => {
                        {
                            let mut guard = self.content.lock().unwrap();
                            guard.clear();
                            guard.push_str(content.as_str());
                            let mut guard = self.current_url.lock().unwrap();
                            *guard = url.clone();
                        }
                        self.ui
                            .read()
                            .unwrap()
                            .ui_tx
                            .read()
                            .unwrap()
                            .send(UiMessage::ShowContent(url, content, item_type, index))
                            .unwrap();
                    }
                    ControllerMessage::ShowMessage(msg) => {
                        self.ui
                            .read()
                            .unwrap()
                            .ui_tx
                            .read()
                            .unwrap()
                            .send(UiMessage::ShowMessage(msg))
                            .unwrap();
                    }
                    ControllerMessage::BinaryWritten(filename, bytes_written) => {
                        self.ui
                            .read()
                            .unwrap()
                            .ui_tx
                            .read()
                            .unwrap()
                            .send(UiMessage::BinaryWritten(filename, bytes_written))
                            .unwrap();
                    }
                    ControllerMessage::NavigateBack => {
                        self.navigate_back();
                    }
                    ControllerMessage::OpenHtml(url) => {
                        self.open_command("html_command", url).unwrap();
                    }
                    ControllerMessage::OpenImage(url) => {
                        self.open_command("image_command", url).unwrap();
                    }
                    ControllerMessage::OpenTelnet(url) => {
                        self.open_command("telnet_command", url).unwrap();
                    }
                    ControllerMessage::FetchGeminiUrl(url, add_to_history, index) => {
                        self.fetch_gemini_url(url, add_to_history, index);
                    }
                    ControllerMessage::FetchUrl(url, item_type, add_to_history, index) => {
                        self.fetch_url(url, item_type, add_to_history, index);
                    }
                    ControllerMessage::FetchBinaryUrl(url, local_path) => {
                        self.fetch_binary_url(url, local_path);
                    }
                    ControllerMessage::RedrawBookmarks => {
                        trace!("Controller: Clearing bookmarks");
                        self.ui
                            .read()
                            .unwrap()
                            .ui_tx
                            .read()
                            .unwrap()
                            .send(UiMessage::ClearBookmarksMenu)
                            .unwrap();
                        for entry in self.bookmarks.lock().unwrap().entries.clone() {
                            self.ui
                                .read()
                                .unwrap()
                                .ui_tx
                                .read()
                                .unwrap()
                                .send(UiMessage::AddToBookmarkMenu(entry))
                                .unwrap();
                        }
                    }
                    ControllerMessage::RedrawHistory => {
                        trace!("Controller: Clearing history");
                        self.ui
                            .read()
                            .unwrap()
                            .ui_tx
                            .read()
                            .unwrap()
                            .send(UiMessage::ClearHistoryMenu)
                            .unwrap();
                        let entries = self
                            .history
                            .lock()
                            .unwrap()
                            .get_latest_history(10)
                            .expect("Could not get latest history");
                        for entry in entries {
                            self.ui
                                .read()
                                .unwrap()
                                .ui_tx
                                .read()
                                .unwrap()
                                .send(UiMessage::AddToHistoryMenu(entry))
                                .unwrap();
                        }
                    }
                };
            }
        }
    }
}
