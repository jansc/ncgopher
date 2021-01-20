use chrono::{Duration, Local, Utc};
use cursive::Cursive;
use sha2::{Digest, Sha256};
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
use crate::certificates::Certificates;
use crate::gemini::GeminiType;
use crate::gophermap::{GopherMapEntry, ItemType};
use crate::history::{History, HistoryEntry};
use crate::ncgopher::{NcGopher, UiMessage};
use crate::SETTINGS;
use native_tls::{Protocol, TlsConnector};
use x509_parser::prelude::*;

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
    /// Known hosts for gemini TOFU
    certificates: Arc<Mutex<Certificates>>,
    /// Current textual content
    content: Arc<Mutex<String>>,
    /// Current URL
    current_url: Arc<Mutex<Url>>,
    /// When the user triggers several requests, only the last request
    /// will be displayed, the other will be canceled.
    last_request_id: Arc<Mutex<i64>>,
}

/// Defines messages sent between Controller and UI
pub enum ControllerMessage {
    AddBookmark(Url, String, String),
    AddCertificate(Url, String),
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
    RequestCertificateChangedDialog(Url, String),
    RequestEditHistoryDialog,
    RequestEditBookmarksDialog,
    RequestGeminiQueryDialog(Url, String, bool),
    RequestSaveAsDialog,
    RequestSettingsDialog,
    SavePageAs(String),
    SetContent(Url, String, ItemType, usize),
    SetGeminiContent(Url, GeminiType, String, usize),
    ShowMessage(String),
    UpdateCertificateAndOpen(Url, String),
    RedrawBookmarks,
    RedrawHistory,
    FetchGeminiUrl(Url, bool, usize),
    FetchUrl(Url, ItemType, bool, usize),
    FetchBinaryUrl(Url, String),
}

impl Controller {
    /// Create a new controller (created in main.rs)
    pub fn new(app: Cursive, url: Url) -> Result<Controller, Box<dyn Error>> {
        let (tx, rx) = mpsc::channel::<ControllerMessage>();
        let mut ncgopher = NcGopher::new(app, tx.clone());

        let controller = Controller {
            rx: Arc::new(rx),
            tx: Arc::new(RwLock::new(tx)),
            ui: Arc::new(RwLock::new(ncgopher.clone())),
            history: Arc::new(Mutex::new(History::new()?)),
            bookmarks: Arc::new(Mutex::new(Bookmarks::new())),
            certificates: Arc::new(Mutex::new(Certificates::new())),
            content: Arc::new(Mutex::new(String::new())),
            current_url: Arc::new(Mutex::new(Url::parse("gopher://host.none").unwrap())),
            last_request_id: Arc::new(Mutex::new(0)),
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
        // Add bookmarks to bookmark menu on startup
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
        // open initial page
        ncgopher.open_url(url, true, 0);
        info!("Controller::new() done");
        Ok(controller)
    }

    // Used for gemini downloads
    fn get_filename_from_url(&self, url: &Url) -> String {
        let download_path = SETTINGS
            .read()
            .unwrap()
            .get_str("download_path")
            .unwrap_or_default();

        if let Some(mut segments) = url.path_segments().map(|c| c.collect::<Vec<_>>()) {
            let filename = segments.pop().unwrap();
            // Get download_path from settings
            let path = Path::new(download_path.as_str()).join(filename);
            return path.display().to_string();
        } else {
            // TODO: Create extension based on mimetype
            // Use download_path from settings
            let path = Path::new(download_path.as_str()).join("download.bin");
            path.display().to_string()
        }
    }

    fn fetch_gemini_url(&self, mut url: Url, add_to_history: bool, index: usize) {
        trace!("Controller::fetch_gemini_url({})", url);
        let tx_clone = self.tx.read().unwrap().clone();

        let request_id = {
            let mut guard = self.last_request_id.lock().unwrap();
            *guard += 1;
            *guard
        };
        let request_id_ref = self.last_request_id.clone();

        normalize_domain(&mut url);

        let host = url.host_str().unwrap().to_string();
        // can only be a gemini URL, no need to check the scheme
        let server_details = url
            .socket_addrs(|| Some(1965))
            .expect("could not understand URL")[0];

        let local_filename = self.get_filename_from_url(&url);

        // Get known certificate fingerprint for host
        let fingerprint = self.certificates.lock().unwrap().get(&url);

        thread::spawn(move || {
            let mut buf = String::new();
            let mut builder = TlsConnector::builder();

            // Self-signed certificates are considered invalid, but they are quite
            // common for gemini servers. Therefore, we accept invalid certs,
            // but check for expiration later
            builder.danger_accept_invalid_certs(true);

            // Rust's native-tls does not yet provide Tlsv13 :(
            // Alternative implementation: rusttls
            builder.min_protocol_version(Some(Protocol::Tlsv12));

            let connector = match builder.build() {
                Ok(connector) => connector,
                Err(err) => {
                    tx_clone
                        .send(ControllerMessage::ShowMessage(format!(
                            "Could not establish connection: {}",
                            err
                        )))
                        .unwrap();
                    return;
                }
            };

            let stream = match TcpStream::connect(&server_details) {
                Ok(stream) => stream,
                Err(err) => {
                    tx_clone
                        .send(ControllerMessage::ShowMessage(format!(
                            "Could not connect to server: {}",
                            err
                        )))
                        .unwrap();
                    return;
                }
            };

            let mut stream = match connector.connect(&host, stream) {
                Ok(stream) => stream,
                Err(err) => {
                    warn!("Could not open tls stream: {} to {}", err, server_details);
                    tx_clone
                        .send(ControllerMessage::ShowMessage(format!(
                            "Could not open tls stream to {}: {}",
                            server_details, err
                        )))
                        .unwrap();
                    return;
                }
            };

            info!("Connected with TLS");

            // try to get peer certificate
            let cert_opt = match stream.peer_certificate() {
                Ok(cert_opt) => cert_opt,
                Err(err) => {
                    tx_clone
                        .send(ControllerMessage::ShowMessage(format!(
                            "Could not get peer certificate: {:?}",
                            err
                        )))
                        .unwrap();
                    return;
                }
            };

            // check certificate
            if let Some(cert) = cert_opt {
                // TOFU: Check if we already have a certificate fingerprint for a given host
                let cert_fingerprint = cert.to_der().unwrap();
                // create a Sha256 object
                let mut hasher = Sha256::new();
                hasher.update(cert_fingerprint);
                let cert_fingerprint = base64::encode(hasher.finalize());

                info!("Peer certificate: {:?}", cert_fingerprint);
                match fingerprint {
                    Some(f) => {
                        if f != cert_fingerprint {
                            // Invalid CERTIFICATE, notify user
                            tx_clone
                                .send(ControllerMessage::ShowMessage(format!(
                                    "Certificate fingerprint DOES NOT match for {}",
                                    url
                                )))
                                .unwrap();
                            tx_clone
                                .send(ControllerMessage::RequestCertificateChangedDialog(
                                    url,
                                    cert_fingerprint,
                                ))
                                .unwrap();
                            return;
                        } else {
                            tx_clone
                                .send(ControllerMessage::ShowMessage(
                                    "Certificate fingerprint matches".to_string(),
                                ))
                                .unwrap();
                        }
                    }
                    None => {
                        // 1st time visit: add fingerprint
                        tx_clone
                            .send(ControllerMessage::AddCertificate(
                                url.clone(),
                                cert_fingerprint,
                            ))
                            .unwrap()
                    }
                }

                // Check certificate expiration date
                match parse_x509_certificate(&cert.to_der().unwrap()) {
                    Ok((_rem, cert)) => {
                        info!("Successfully parsed certificate");
                        match cert.tbs_certificate.validity.time_to_expiration() {
                            Some(duration) => {
                                let now = Utc::now();
                                let expires =
                                    now.checked_add_signed(Duration::from_std(duration).unwrap());
                                match expires {
                                    Some(x) => info!("Certificate expires {}", x.to_rfc3339()),
                                    None => warn!("Certificate expire date overflows!"),
                                }

                                info!("Certificate valid {:?}", duration);
                            }
                            None => {
                                tx_clone
                                    .send(ControllerMessage::ShowMessage(
                                        "Server certificate expired.".to_string(),
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
            }

            // Handshake done, request URL from gemini server
            info!("Writing url '{}'", url.as_str());
            stream.write_all(format!("{}\r\n", url).as_bytes()).unwrap();

            let mut bufr = BufReader::new(stream);
            info!("Reading from gemini stream");
            // Read Gemini Header
            match bufr.read_line(&mut buf) {
                Ok(_) => (),
                Err(e) => {
                    tx_clone
                        .send(ControllerMessage::ShowMessage(format!("I/O error: {}", e)))
                        .unwrap();
                    return;
                }
            }
            let buf = buf.trim();
            // "text/gemini; charset=utf-8"
            info!("Got gemini header: {}:  {}", buf.len(), buf);

            {
                // Abort request, if user triggered a newer request
                let guard = request_id_ref.lock().unwrap();
                if request_id < *guard {
                    return;
                }
            }

            if buf.is_empty() {
                tx_clone
                    .send(ControllerMessage::ShowMessage(
                        "Could not read from stream".to_string(),
                    ))
                    .unwrap();
                return;
            }

            // <META> always starts at the 4th char
            // (it might contain leading whitespace)
            let meta = buf.chars().skip(3).collect::<String>();
            // <META> has a maximum size
            if meta.len() > 1024 {
                tx_clone
                    .send(ControllerMessage::ShowMessage(
                        "invalid header from server: <META> too large".to_string(),
                    ))
                    .unwrap();
            }

            // A function to check the second digit of a status code in the default
            // branch. I.e. the second digit should be zero.
            //
            // Returns false if the status code is invalid and thus the response
            // header is invalid.
            let check = |other: Option<char>| -> bool {
                if other == Some('0') {
                    // ok
                } else if matches!(other, Some(c) if c.is_ascii_digit()) {
                    // the second char is an ASCII digit, but this code is not handled
                    tx_clone
                        .send(ControllerMessage::ShowMessage(format!(
                            "unknown status code {}",
                            buf.chars().take(2).collect::<String>(),
                        )))
                        .unwrap();
                } else {
                    // either the second char is not an ASCII digit
                    // or does not exist at all
                    tx_clone
                        .send(ControllerMessage::ShowMessage(format!(
                            "invalid header from server: invalid status code: {}",
                            buf
                        )))
                        .unwrap();
                    // the header is already invalid, no need to check further
                    return false;
                }
                // after the two digit status code there should be a space
                // otherwhise the header is invalid too
                buf.chars().nth(2) == Some(' ')
            };

            match buf.chars().next() {
                Some('1') => {
                    // INPUT
                    match buf.chars().nth(1) {
                        Some('1') => {
                            tx_clone
                                .send(
                                    ControllerMessage::RequestGeminiQueryDialog(
                                        url, meta, true,
                                    ),
                                )
                                .unwrap();
                        }
                        other => {
                            if check(other) {
                                info!("Got query: {}", meta);
                                tx_clone
                                    .send(
                                        ControllerMessage::RequestGeminiQueryDialog(
                                            url, meta, false,
                                        ),
                                    )
                                    .unwrap();
                            }
                            tx_clone
                                .send(ControllerMessage::RedrawHistory)
                                .unwrap();
                        }
                    }
                }
                Some('2') => {
                    // SUCCESS
                    // there are not yet any other status codes
                    // than 20 in this category
                    if check(buf.chars().nth(1)) {
                        // If mimetype is text/* download as gemini
                        // Otherwise initiate a binary download
                        // FIXME: for now assumes all text is gemini text
                        if meta.starts_with("text/") {
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
                                    url.clone(),
                                    GeminiType::Gemini,
                                    s.to_string(),
                                    index,
                                ))
                                .unwrap();
                            if add_to_history {
                                tx_clone
                                    .send(ControllerMessage::AddToHistory(
                                        url.clone(),
                                    ))
                                    .unwrap();
                                tx_clone
                                    .send(ControllerMessage::RedrawHistory)
                                    .unwrap();
                            }
                        } else {
                            // Binary download
                            let f = File::create(local_filename.clone()).unwrap_or_else(
                                |_| {
                                    panic!(
                                        "Unable to open file '{}'",
                                        local_filename.clone()
                                    )
                                }
                            );
                            let mut bw = BufWriter::new(f);
                            let mut buf = [0u8; 1024];
                            let mut total_written = 0;
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
                    }
                }
                Some('3') => {
                    // REDIRECT
                    let other = buf.chars().nth(1);
                    if other == Some('1') {
                        // redirect is permanent
                        // TODO: Should automatically update bookmarks
                    } else if !check(other) {
                        return;
                    }
                    match Url::parse(&meta) {
                        Ok(url) => {
                            // FIXME: Try to parse url, check scheme
                            tx_clone.send(ControllerMessage::FetchGeminiUrl(url,true,0)).unwrap();
                        }
                        Err(_) => {
                            tx_clone
                                .send(ControllerMessage::ShowMessage(format!(
                                    "invalid redirect url: {}",
                                    meta
                                )))
                                .unwrap();
                        }
                    }
                }
                Some('4') // FAILURE
                | Some('5') // PERMANENT FAILURE
                | Some('6') // CLIENT CERTIFICATE
                => {
                    if check(buf.chars().nth(1)) {
                        // reset content and set current URL for retrying
                        tx_clone
                            .send(ControllerMessage::SetGeminiContent(
                                url,
                                GeminiType::Text,
                                String::new(),
                                0,
                            )).unwrap();
                        tx_clone
                            .send(ControllerMessage::ShowMessage(format!(
                                "Gemini error: {}",
                                buf
                            )))
                            .unwrap();
                    }
                }
                other => {
                    tx_clone
                        .send(ControllerMessage::ShowMessage(if other.is_some() {
                            format!("invalid header from server: invalid status code: {}", buf)
                        } else {
                            format!("invalid header from server: missing status code: {}", buf)
                        }))
                        .unwrap();
                }
            }
            info!("finished reading from gemini stream");
        });
    }

    fn fetch_url(&self, url: Url, item_type: ItemType, add_to_history: bool, index: usize) {
        // index is the position in the text (used when navigatin back or reloading)
        trace!("Controller::fetch_url({})", url);
        let tx_clone = self.tx.read().unwrap().clone();
        let request_id = {
            let mut guard = self.last_request_id.lock().unwrap();
            *guard += 1;
            *guard
        };
        let request_id_ref = self.last_request_id.clone();

        let port = url.port().unwrap_or(70);
        let server = url
            .host()
            .map_or("host.error".to_string(), |host| host.to_string());
        let path = url.path();
        let mut path = str::replace(path, "%09", "\t");
        info!("fetch_url(): About to open URL {}", path);
        if path.len() > 2 {
            // TODO: check x[0] == / and x[1] == itemtype
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
            // TLS-support. If non-standard-port, try to connect with TLS
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
                let guard = request_id_ref.lock().unwrap();
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
                    url.clone(),
                    s.to_string(),
                    item_type,
                    index,
                ))
                .unwrap();
        });
    }

    fn fetch_binary_url(&self, url: Url, local_filename: String) {
        let tx_clone = self.tx.read().unwrap().clone();

        let port = url.port().unwrap_or(70);
        let server = url
            .host()
            .map_or("host.error".to_string(), |host| host.to_string());
        let path = if url.path().len() > 2 {
            // TODO: check x[0] == / and x[1] == itemtype
            url.path()[2..].to_string()
        } else {
            String::new()
        };

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
                .unwrap_or_else(|_| panic!("Unable to open file '{}'", local_filename));
            let mut bw = BufWriter::new(f);
            let mut buf = [0u8; 1024];
            let mut total_written = 0;
            if port != 70 {
                if let Ok(connector) = TlsConnector::new() {
                    let stream = TcpStream::connect(server_details.clone()).unwrap_or_else(|_| {
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
    // open_gopher_address

    fn add_bookmark(&mut self, url: Url, title: String, tags: String) -> Bookmark {
        let tags = tags.as_str().split_whitespace().map(String::from).collect();
        let b = Bookmark { title, url, tags };
        // Check if bookmark exists
        if self.bookmarks.lock().unwrap().exists(b.clone().url) {
            self.bookmarks.lock().unwrap().remove(b.clone().url);
        }
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
        let h = HistoryEntry {
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
        let content = self.content.lock().unwrap().clone();
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

    fn save_gemini(&mut self, filename: String) {
        let gemini_content = self.content.lock().unwrap().clone();
        let tx_clone = self.tx.read().unwrap().clone();
        let lines = gemini_content
            .lines()
            .map(str::to_string)
            .collect::<Vec<String>>();
        info!("Save textfile: {}", filename);

        // Create a path to the desired file
        let download_path = SETTINGS
            .read()
            .unwrap()
            .get_str("download_path")
            .unwrap_or_default();

        let path = Path::new(download_path.as_str()).join(filename.as_str());
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
        for l in lines {
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

    /// Save the current gophermap to disk
    fn save_gophermap(&mut self, filename: String) {
        let content = self.content.lock().unwrap().clone();
        let tx_clone = self.tx.read().unwrap().clone();
        let mut txtlines = Vec::new();
        for l in content.lines().skip(1) {
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
        let download_path = SETTINGS
            .read()
            .unwrap()
            .get_str("download_path")
            .unwrap_or_default();

        let path = Path::new(download_path.as_str()).join(filename.as_str());
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
                    ControllerMessage::AddCertificate(url, fingerprint) => {
                        self.certificates
                            .lock()
                            .expect("could not lock certificate store")
                            .insert(&url, fingerprint);
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
                        let current_url = self.current_url.lock().unwrap().clone();
                        let index = self
                            .ui
                            .read()
                            .unwrap()
                            .get_selected_item_index()
                            .unwrap_or(0);
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
                        let current_url = self.current_url.lock().unwrap().clone();
                        let bookmark = Bookmark {
                            title: String::new(),
                            url: current_url.clone(),
                            tags: Vec::new(),
                        };
                        self.ui
                            .read()
                            .unwrap()
                            .ui_tx
                            .read()
                            .unwrap()
                            .send(UiMessage::ShowAddBookmarkDialog(bookmark))
                            .unwrap();
                    }
                    ControllerMessage::RequestCertificateChangedDialog(url, fingerprint) => {
                        info!("RequestCertificateChangedDialog({}, {})", url, fingerprint);
                        self.ui
                            .read()
                            .unwrap()
                            .ui_tx
                            .read()
                            .unwrap()
                            .send(UiMessage::ShowCertificateChangedDialog(url, fingerprint))
                            .unwrap();
                    }
                    ControllerMessage::RequestEditHistoryDialog => {
                        let entries = self
                            .history
                            .lock()
                            .unwrap()
                            .get_latest_history(500)
                            .expect("Could not get latest history");
                        self.ui
                            .read()
                            .unwrap()
                            .ui_tx
                            .read()
                            .unwrap()
                            .send(UiMessage::ShowEditHistoryDialog(entries))
                            .unwrap();
                    }
                    ControllerMessage::RequestEditBookmarksDialog => {
                        let v = self.bookmarks.lock().unwrap().clone().entries;
                        self.ui
                            .read()
                            .unwrap()
                            .ui_tx
                            .read()
                            .unwrap()
                            .send(UiMessage::ShowEditBookmarksDialog(v))
                            .unwrap();
                    }
                    ControllerMessage::RequestGeminiQueryDialog(url, query, secret) => {
                        self.ui
                            .read()
                            .unwrap()
                            .ui_tx
                            .read()
                            .unwrap()
                            .send(UiMessage::OpenGeminiQueryDialog(url, query, secret))
                            .unwrap();
                    }
                    ControllerMessage::RequestSaveAsDialog => {
                        let current_url = self.current_url.lock().unwrap().clone();
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
                        let url = self.current_url.lock().unwrap().clone();

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
                    ControllerMessage::SetGeminiContent(url, gemini_type, content, index) => {
                        {
                            let mut guard = self.content.lock().unwrap();
                            guard.clear();
                            guard.push_str(content.as_str());
                            *self.current_url.lock().unwrap() = url.clone();
                        }
                        self.ui
                            .read()
                            .unwrap()
                            .ui_tx
                            .read()
                            .unwrap()
                            .send(UiMessage::ShowGeminiContent(
                                url,
                                gemini_type,
                                content,
                                index,
                            ))
                            .unwrap();
                    }
                    ControllerMessage::SetContent(url, content, item_type, index) => {
                        {
                            let mut guard = self.content.lock().unwrap();
                            guard.clear();
                            guard.push_str(content.as_str());
                        }
                        {
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
                    ControllerMessage::UpdateCertificateAndOpen(url, fingerprint) => {
                        self.certificates
                            .lock()
                            .expect("could not lock certificate store")
                            .insert(&url, fingerprint);
                        self.ui
                            .read()
                            .unwrap()
                            .ui_tx
                            .read()
                            .unwrap()
                            .clone()
                            .send(UiMessage::OpenUrl(url.clone(), true, 0))
                            .unwrap()
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

pub fn normalize_domain(u: &mut Url) {
    use idna::domain_to_ascii;
    use percent_encoding::percent_decode_str;

    // remove default port number
    if u.port() == Some(1965) {
        u.set_port(None).expect("gemini URL without host");
    }

    if let Some(domain) = u.domain() {
        // since the gemini scheme is not "special" according to the WHATWG spec
        // it will be percent-encoded by the url crate which has to be undone
        let domain = percent_decode_str(domain)
            .decode_utf8()
            .expect("could not decode percent-encoded url");
        // reencode the domain as IDNA
        let domain = domain_to_ascii(&domain).expect("could not IDNA encode URL");
        // make the url use the newly encoded domain name
        u.set_host(Some(&domain)).expect("error replacing host");
    } else {
        log::info!("tried to reencode URL to IDNA that did not contain a domain name");
    }
}
