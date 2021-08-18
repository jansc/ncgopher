use chrono::{Duration, Local, Utc};
use cursive::{
    utils::{lines::simple::LinesIterator, markup::StyledString},
    view::{Nameable, Resizable},
    views::{Dialog, EditView, SelectView},
    Cursive, CursiveRunnable,
};
use mime::Mime;
use native_tls::{Protocol, TlsConnector};
use sha2::{Digest, Sha256};
use std::error::Error;
use std::fs::OpenOptions;
use std::io::{BufRead, BufReader, BufWriter, Read, Write};
use std::net::TcpStream;
use std::path::Path;
use std::process::Command;
use std::sync::{Arc, Mutex, RwLock};
use std::thread;
use url::Url;
use x509_parser::prelude::*;

use crate::bookmarks::{Bookmark, Bookmarks};
use crate::certificates::Certificates;
use crate::gemini::GeminiType;
use crate::gophermap::{GopherMapEntry, ItemType};
use crate::history::{History, HistoryEntry};
use crate::ui::layout::Layout;
use crate::url_tools::{download_filename_from_url, human_readable_url, normalize_domain};
use crate::SETTINGS;

#[derive(Clone, Debug)]
pub enum Direction {
    Next,
    Previous,
}

const HISTORY_LEN: usize = 10;

#[derive(Clone)]
pub struct Controller {
    sender: crossbeam_channel::Sender<Box<dyn FnOnce(&mut Cursive) + 'static + Send>>,
    /// The browsing history
    pub(crate) history: Arc<Mutex<History>>,
    /// Bookmarks
    pub(crate) bookmarks: Arc<Mutex<Bookmarks>>,
    /// Known hosts for gemini TOFU
    certificates: Arc<Mutex<Certificates>>,
    /// Current textual content
    content: Arc<Mutex<String>>,
    /// Current URL
    pub current_url: Arc<Mutex<Url>>,
    /// When the user triggers several requests, only the last request
    /// will be displayed, the other will be canceled.
    last_request_id: Arc<Mutex<i64>>,
    /// Message shown in statusbar
    message: Arc<RwLock<String>>,
}

impl Controller {
    /// Create a new controller (created in main.rs)
    pub fn setup(app: &mut CursiveRunnable, url: Url) -> Result<(), Box<dyn Error>> {
        crate::ui::setup::setup(app);

        let mut controller = Controller {
            sender: app.cb_sink().clone(),
            history: Arc::new(Mutex::new(History::new()?)),
            bookmarks: Arc::new(Mutex::new(Bookmarks::new())),
            certificates: Arc::new(Mutex::new(Certificates::new())),
            content: Arc::new(Mutex::new(String::new())),
            current_url: Arc::new(Mutex::new(Url::parse("about:blank").unwrap())),
            last_request_id: Arc::new(Mutex::new(0)),
            message: app
                .find_name::<crate::ui::statusbar::StatusBar>("statusbar")
                .unwrap()
                .get_message(),
        };

        // Add old entries to history on start-up
        let menutree = app
            .menubar()
            .find_subtree("History")
            .expect("history menu missing");
        let entries = controller
            .history
            .lock()
            .unwrap()
            .get_latest_history(HISTORY_LEN)
            .expect("Could not get latest history");
        for entry in entries {
            let title = entry.title.clone();
            menutree.insert_leaf(3, title, move |app| {
                app.user_data::<Controller>()
                    .expect("controller missing")
                    .open_url(entry.url.clone(), true, 0);
            });
        }

        // Add bookmarks to bookmark menu on startup
        info!("Adding existing bookmarks to menu");
        let menutree = app
            .menubar()
            .find_subtree("Bookmarks")
            .expect("bookmarks menu missing");
        let entries = controller.bookmarks.lock().unwrap().get_bookmarks();
        for entry in entries {
            let url = entry.url.clone();
            menutree.insert_leaf(3, &entry.title, move |app| {
                app.user_data::<Controller>()
                    .expect("controller missing")
                    .open_url(url.clone(), true, 0);
            });
        }

        // open initial page
        controller.open_url(url, true, 0);

        app.set_user_data(controller);

        info!("Controller::new() done");

        Ok(())
    }

    fn fetch_gemini_url(&self, mut url: Url, index: usize) {
        trace!("Controller::fetch_gemini_url({})", url);

        let request_id = {
            let mut guard = self.last_request_id.lock().unwrap();
            *guard += 1;
            *guard
        };
        let request_id_ref = self.last_request_id.clone();

        normalize_domain(&mut url);

        let host = url.host_str().unwrap().to_string();
        // can only be a gemini URL, no need to check the scheme
        let server_details = match url.socket_addrs(|| Some(1965)) {
            Ok(sock_addrs) => sock_addrs[0],
            Err(err) => {
                self.set_message(&format!("invalid URL: {}", err));
                return;
            }
        };

        // Get known certificate fingerprint for host
        let fingerprint = self.certificates.lock().unwrap().get(&url);
        let message = self.message.clone();
        let sender = self.sender.clone();

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
                    *message.write().unwrap() = format!("Could not establish connection: {}", err);
                    return;
                }
            };

            let stream = match TcpStream::connect(&server_details) {
                Ok(stream) => stream,
                Err(err) => {
                    *message.write().unwrap() = format!("Could not connect to server: {}", err);
                    return;
                }
            };

            let mut stream = match connector.connect(&host, stream) {
                Ok(stream) => stream,
                Err(err) => {
                    warn!("Could not open tls stream: {} to {}", err, server_details);
                    *message.write().unwrap() =
                        format!("Could not open tls stream to {}: {}", server_details, err);
                    return;
                }
            };

            info!("Connected with TLS");

            // try to get peer certificate
            let cert_opt = match stream.peer_certificate() {
                Ok(cert_opt) => cert_opt,
                Err(err) => {
                    *message.write().unwrap() =
                        format!("Could not get peer certificate: {:?}", err);
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
                            // Invalid certificate, notify user
                            *message.write().unwrap() =
                                format!("Certificate fingerprint DOES NOT match for {}", url);
                            sender
                                .send(Box::new(move |app| {
                                    crate::ui::dialogs::certificate_changed(
                                        app,
                                        url,
                                        cert_fingerprint,
                                    );
                                }))
                                .unwrap();
                            return;
                        } else {
                            *message.write().unwrap() =
                                format!("Certificate fingerprint matches for {}", url);
                        }
                    }
                    None => {
                        // 1st time visit: add fingerprint
                        let url = url.clone();
                        sender
                            .send(Box::new(move |app| {
                                Controller::certificate_changed_action(app, &url, cert_fingerprint);
                            }))
                            .unwrap();
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
                                *message.write().unwrap() = "Server certificate expired.".into()
                            }
                        }
                    }
                    Err(err) => {
                        *message.write().unwrap() =
                            format!("Could not parse peer certificate: {:?}", err)
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
                    *message.write().unwrap() = format!("I/O error: {}", e);
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
                *message.write().unwrap() = "Could not read from stream".into();
                return;
            }

            // <META> always starts at the 4th char
            // (it might contain leading whitespace)
            let meta = buf.chars().skip(3).collect::<String>();
            // <META> has a maximum size
            if meta.len() > 1024 {
                *message.write().unwrap() = "Invalid header from server: <META> too large".into();
                return;
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
                    *message.write().unwrap() = format!(
                        "unknown status code {}",
                        buf.chars().take(2).collect::<String>(),
                    );
                } else {
                    // either the second char is not an ASCII digit
                    // or does not exist at all
                    *message.write().unwrap() =
                        format!("invalid header from server: invalid status code: {}", buf);
                    // the header is already invalid, no need to check further
                    return false;
                }
                // after the two digit status code there should be a space
                // otherwhise the header is invalid too
                if buf.chars().nth(2) != Some(' ') {
                    if matches!(buf.chars().nth(2), Some(c) if c.is_whitespace()) {
                        // not space, but still whitespace
                        info!("header is invalid, but recoverable: {:?}", buf);
                    } else {
                        // really no idea what this is
                        *message.write().unwrap() =
                            format!("invalid header from server: malformed: {}", buf);
                        return false;
                    }
                }
                true
            };

            match buf.chars().next() {
                Some('1') => {
                    // INPUT
                    let secret = match buf.chars().nth(1) {
                        Some('1') => true,
                        other => {
                            if !check(other){
                                return
                            } else {
                                false
                            }
                        },
                    };
                    sender.send(Box::new(move |app|{
                        crate::ui::dialogs::gemini_query(app, url, meta, secret);
                    })).unwrap();
                }
                Some('2') => {
                    // SUCCESS
                    // there are not yet any other status codes
                    // than 20 in this category
                    if !check(buf.chars().nth(1)) {
                        return;
                    }

                    let mime = meta.parse::<Mime>()
                        .unwrap_or_else(|_| "text/gemini".parse().unwrap());

                    if mime.type_() == "text" {
                        // some kind of text. First check encoding.
                        let encoding = mime.get_param("charset")
                            // default is UTF-8
                            .map_or("utf-8", |param| param.as_str())
                            // charset identifiers are case-insensitive
                            .to_lowercase();

                        if !matches!(encoding.as_str(),
                            // IANA has many aliases for ASCII
                            // https://www.iana.org/assignments/character-sets/character-sets.xhtml
                            // since it's a strict subset of UTF-8 we can read it
                            "us-ascii" | "iso-ir-6" | "ansi_x3.4-1968"
                            | "ansi_x3.4-1986" | "iso_646.rv:1991"
                            | "iso646-us" | "us" | "IBM367" | "cp367"
                            | "csascii"
                            // UTF-8, also allow a nonstandard spelling
                            | "utf-8" | "csutf8" | "utf8")
                        {
                            // not UTF-8 or ASCII, encoding not supported
                            sender.send(Box::new(move |app| {
                                app.add_layer(Dialog::info(format!("The page you tried to access is encoded as \"{}\". This encoding is not supported by ncgopher.", encoding)))
                            })).unwrap();
                            return;
                        }
                        // if we get this far, it has to be UTF-8/ASCII

                        let mut buf = vec![];
                        bufr.read_to_end(&mut buf).unwrap_or_else(|err| {
                            *message.write().unwrap() = format!(
                                "I/O error: {}",
                                err
                            );
                            0
                        });

                        let gemini_type = match mime.subtype().as_str() {
                            "gemini" => GeminiType::Gemini,
                            // FIXME: add HTML handler
                            _ => GeminiType::Text,
                        };

                        let s = String::from_utf8_lossy(&buf).into_owned();
                        sender.send(Box::new(move |app|{
                            let controller = app.user_data::<Controller>().expect("controller missing");
                            controller.set_message(url.as_str());
                            controller.set_gemini_content(url, gemini_type, s, index);
                        })).unwrap();
                    } else {
                        // Binary download
                        let local_filename = download_filename_from_url(&url);
                        let open = OpenOptions::new()
                            .write(true)
                            // make sure to not clobber downloaded files
                            .create_new(true)
                            .open(&local_filename);

                        match open {
                            Ok(file) => {
                                let mut bw = BufWriter::new(file);
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
                                    *message.write().unwrap() = format!(
                                        "{} bytes read",
                                        total_written
                                    );
                                }
                                *message.write().unwrap() = format!("File downloaded: {} ({} bytes)", local_filename, total_written);
                            }
                            Err(err) => {
                                *message.write().unwrap() = format!("Unable to open file '{}': {}", local_filename, err);
                            }
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
                    // redirect might be relative
                    match url.join(&meta) {
                        Ok(url) => {
                            // FIXME: Try to parse url, check scheme
                            // FIXME: limit number of redirects
                            sender.send(Box::new(move |app|{
                                let controller = app.user_data::<Controller>().expect("controller missing");
                                controller.open_url(url, true, 0);
                            })).unwrap();
                        }
                        Err(_) => {
                            *message.write().unwrap() = format!(
                                "invalid redirect url: {}",
                                meta
                            );
                        }
                    }
                }
                Some('4') // FAILURE
                | Some('5') // PERMANENT FAILURE
                | Some('6') // CLIENT CERTIFICATE
                => {
                    if check(buf.chars().nth(1)) {
                        let header = buf.to_string();
                        sender.send(Box::new(move |app|{
                            let controller = app.user_data::<Controller>().expect("controller missing");
                            // reset content and set current URL for retrying
                            controller.set_gemini_content(url, GeminiType::Text, String::new(), 0);
                            controller.set_message(&format!("Gemini error: {}", header));
                        })).unwrap();
                    }
                }
                other => {
                    *message.write().unwrap() = if other.is_some() {
                        format!("invalid header from server: invalid status code: {}", buf)
                    } else {
                        format!("invalid header from server: missing status code: {}", buf)
                    };
                }
            }
            info!("finished reading from gemini stream");
        });
    }

    fn fetch_url(&self, url: Url, item_type: ItemType, index: usize) {
        // index is the position in the text (used when navigatin back or reloading)
        trace!("Controller::fetch_url({})", url);

        let request_id = {
            let mut guard = self.last_request_id.lock().unwrap();
            *guard += 1;
            *guard
        };

        let port = url.port().unwrap_or(70);
        let server = url.host_str().expect("no host").to_string();
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

        let request_id_ref = self.last_request_id.clone();
        let message = self.message.clone();
        let sender = self.sender.clone();

        thread::spawn(move || {
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
                                        *message.write().unwrap() = format!("I/O error: {}", e);
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
                                    *message.write().unwrap() = format!("I/O error: {}", e);
                                }
                            }
                        }
                    }
                    Err(e) => {
                        *message.write().unwrap() = format!("Couldn't connect to server: {}", e);
                        return;
                    }
                };
            }

            let guard = request_id_ref.lock().unwrap();
            if request_id < *guard {
                return;
            }
            drop(guard);

            let s = String::from_utf8_lossy(&buf).into_owned();
            sender
                .send(Box::new(move |app| {
                    let controller = app.user_data::<Controller>().expect("controller missing");
                    controller.set_message(url.as_str());
                    controller.set_gopher_content(item_type, s, index);
                }))
                .unwrap();
        });
    }

    fn fetch_binary_url(&mut self, url: Url, local_filename: String) {
        self.set_message("Downloading binary file...");

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
        let message = self.message.clone();

        thread::spawn(move || {
            // FIXME: Error handling!
            let mut tls = false;
            let open = OpenOptions::new()
                .write(true)
                // make sure to not clobber downloaded files
                .create_new(true)
                .open(local_filename.clone());

            match open {
                Ok(file) => {
                    let mut bw = BufWriter::new(file);
                    let mut buf = [0u8; 1024];
                    let mut total_written = 0;
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
                                        *message.write().unwrap() =
                                            format!("{} bytes read", total_written);
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
                            let bytes_read =
                                stream.read(&mut buf).expect("Could not read from TCP");
                            if bytes_read == 0 {
                                break;
                            }
                            let bytes_written = bw
                                .write(&buf[..bytes_read])
                                .expect("Could not write to file");
                            total_written += bytes_written;
                            *message.write().unwrap() = format!("{} bytes read", total_written);
                        }
                    }
                    *message.write().unwrap() = format!(
                        "File downloaded: {} ({} bytes)",
                        local_filename, total_written
                    );
                }
                Err(err) => {
                    *message.write().unwrap() =
                        format!("Unable to open file '{}': {}", local_filename, err);
                }
            }
        });
    }

    pub fn open_url(&mut self, url: Url, add_to_history: bool, index: usize) {
        info!("Open_url: {} position {}", url, index);
        if add_to_history {
            self.add_to_history(url.clone(), index);
        }
        *self.current_url.lock().unwrap() = url.clone();
        match url.scheme() {
            "gopher" => self.open_gopher_address(url.clone(), ItemType::from_url(&url), index),
            "gemini" => self.open_gemini_address(url.clone(), index),
            "about" => self.open_about(url.clone()),
            "http" | "https" => self.open_command("html_command", url.clone()).unwrap(),
            scheme => self.set_message(&format!("unknown scheme {}", scheme).as_str()),
        }
    }

    /// Show an internal page from the "about" URL scheme
    /// as defined in RFC 6694.
    fn open_about(&mut self, url: Url) {
        let content = match url.path() {
            "blank" => String::new(),
            "help" => include_str!("about/help.gmi").into(),
            "sites" => include_str!("about/sites.gmi").into(),
            "error" => "An error occured.".into(),
            "license" => concat!(
                include_str!("about/license_header.gmi"),
                include_str!("../LICENSE")
            )
            .into(),
            other => {
                self.set_message(&format!("The about page {} does not exist", other));
                return;
            }
        };
        self.set_message(&format!("about:{}", url.path()));
        self.set_gemini_content(url, GeminiType::Gemini, content, 0);
    }

    pub fn open_gopher_address(&mut self, url: Url, item_type: ItemType, index: usize) {
        self.set_message("Loading ...");
        if item_type.is_download() {
            let filename = download_filename_from_url(&url);
            self.fetch_binary_url(url, filename);
        } else {
            self.fetch_url(url, item_type, index);
        }
    }

    /// Renders a gophermap
    fn set_gopher_content(&mut self, item_type: ItemType, content: String, index: usize) {
        let mut guard = self.content.lock().unwrap();
        guard.clear();
        guard.push_str(content.as_str());
        drop(guard);

        if item_type.is_text() {
            self.set_gemini_content(
                Url::parse("about:blank").unwrap(),
                GeminiType::Text,
                content,
                index,
            );
            return;
        }

        // ensure gopher view is focused before setting content
        self.sender
            .send(Box::new(|app| {
                let mut layout = app
                    .find_name::<Layout>("main")
                    .expect("main layout missing");
                layout.set_view("content");
                let human_url = human_readable_url(
                    &app.user_data::<Controller>()
                        .expect("controller missing")
                        .current_url
                        .lock()
                        .unwrap(),
                );
                layout.set_title("content".into(), human_url);
            }))
            .unwrap();

        self.sender
            .send(Box::new(move |app| {
                let textwrap = SETTINGS
                    .read()
                    .unwrap()
                    .get_str("textwrap")
                    .map_or(usize::MAX, |txt| txt.parse().unwrap_or(usize::MAX));

                let viewport_width = app.screen_size().x
                // adjust for left margin
                - 7;

                let viewport_width = std::cmp::min(textwrap, viewport_width);

                let mut view = app
                    .find_name::<SelectView<GopherMapEntry>>("content")
                    .expect("gopher content view missing");
                view.clear();
                let lines = content.lines();
                let mut gophermap = Vec::new();
                let mut first = true;
                for l in lines {
                    if first {
                        if l.starts_with('/') {
                            app.find_name::<Layout>("main")
                                .expect("main layout missing")
                                .set_title("content".into(), l.into());
                        }
                        first = false;
                    }
                    if l != "." {
                        match GopherMapEntry::parse(l.to_string()) {
                            Ok(gl) => {
                                gophermap.push(gl);
                            }
                            Err(err) => {
                                warn!("Invalid gophermap line: {}", err);
                            }
                        };
                    }
                }
                for l in gophermap {
                    let entry = l.clone();

                    let label = entry.clone().label();
                    if entry.item_type == ItemType::Inline && label.len() > viewport_width {
                        for row in LinesIterator::new(&label, viewport_width) {
                            let mut formatted = StyledString::new();
                            let label = format!(
                                "{}  {}",
                                ItemType::as_str(entry.item_type),
                                &label[row.start..row.end]
                            );
                            formatted.append(label);
                            view.add_item(formatted, l.clone());
                        }
                    } else {
                        let mut formatted = StyledString::new();
                        let label =
                            format!("{}  {}", ItemType::as_str(entry.item_type), entry.label());
                        formatted.append(label);
                        view.add_item(formatted, l.clone());
                    }
                }
                view.set_on_submit(|app, entry| {
                    let controller = app.user_data::<Controller>().expect("controller missing");
                    if entry.item_type.is_download()
                        || entry.item_type.is_text()
                        || entry.item_type.is_dir()
                    {
                        controller.open_url(entry.url.clone(), true, 0);
                    } else if entry.item_type.is_query() {
                        // open query dialog
                        let url = entry.url.clone();
                        app.add_layer(
                            Dialog::new()
                                .title("Enter query:")
                                .content(
                                    EditView::new()
                                        // Call `show_popup` when the user presses `Enter`
                                        //FIXME: create closure with url: .on_submit(search)
                                        .with_name("query")
                                        .fixed_width(30),
                                )
                                .button("Cancel", |app| {
                                    app.pop_layer();
                                })
                                .button("Ok", move |app| {
                                    let mut url = url.clone();
                                    let name =
                                        app.find_name::<EditView>("query").unwrap().get_content();
                                    let mut path = url.path().to_string();
                                    path.push_str("%09");
                                    path.push_str(&*name);
                                    url.set_path(path.as_str());

                                    app.pop_layer(); // Close search dialog
                                    let controller =
                                        app.user_data::<Controller>().expect("controller missing");
                                    controller.set_message("Loading ...");
                                    controller.fetch_url(url, ItemType::Dir, 0);
                                }),
                        );
                    } else if entry.item_type.is_html() {
                        controller
                            .open_command("html_command", entry.url.clone())
                            .unwrap();
                    } else if entry.item_type.is_image() {
                        controller
                            .open_command("image_command", entry.url.clone())
                            .unwrap();
                    } else if entry.item_type.is_telnet() {
                        controller
                            .open_command("telnet_command", entry.url.clone())
                            .unwrap();
                    }
                });
                view.set_selection(index);
            }))
            .unwrap();
    }

    fn open_gemini_address(&mut self, url: Url, index: usize) {
        self.set_message("Loading ...");
        self.fetch_gemini_url(url, index);
    }

    fn set_gemini_content(
        &mut self,
        url: Url,
        gemini_type: GeminiType,
        content: String,
        index: usize,
    ) {
        let mut guard = self.content.lock().unwrap();
        guard.clear();
        guard.push_str(content.as_str());
        drop(guard);

        let human_url = human_readable_url(&url);
        // ensure gemini view is focused before setting content
        self.sender
            .send(Box::new(move |app| {
                // set title
                let mut layout = app
                    .find_name::<Layout>("main")
                    .expect("main layout missing");
                layout.set_view("gemini_content");
                layout.set_title("gemini_content".into(), human_url);
                info!("set gemini view");
            }))
            .unwrap();

        self.sender
            .send(Box::new(move |app| {
                let textwrap = SETTINGS
                    .read()
                    .unwrap()
                    .get_str("textwrap")
                    .map_or(usize::MAX, |txt| txt.parse().unwrap_or(usize::MAX));

                let viewport_width = app.screen_size().x
                // adjust for left margin
                - 8;

                let viewport_width = std::cmp::min(textwrap, viewport_width);

                let mut view = app
                    .find_name::<SelectView<Option<Url>>>("gemini_content")
                    .expect("gemini content view missing");
                view.clear();

                if gemini_type == GeminiType::Text {
                    view.add_all(
                        LinesIterator::new(&content, viewport_width)
                            .map(|row| (&content[row.start..row.end], None))
                            .collect::<Vec<_>>(),
                    );
                } else {
                    view.add_all(crate::gemini::parse(&content, &url, viewport_width));
                }
                view.set_on_submit(|app, entry| {
                    if let Some(url) = entry {
                        app.user_data::<Controller>()
                            .expect("controller missing")
                            .open_url(url.clone(), true, 0)
                    }
                });
                view.set_selection(index);
            }))
            .unwrap();
    }

    fn add_to_history(&mut self, url: Url, index: usize) {
        // Updates the position of the last item on the stack This
        // works because add_to_history is called _before_
        // set_content.
        info!(
            "add_to_history(): updating last item's position to {} (URL: {})",
            index, url
        );

        self.sender
            .send(Box::new(move |app| {
                let idx = Controller::get_selected_item_index(app);
                let controller = app.user_data::<Controller>().expect("controller missing");
                let mut guard = controller.history.lock().unwrap();
                guard.update_selected_item(idx);
                drop(guard);
                info!("add_to_history(): {}", url);
                let h = HistoryEntry {
                    title: url.to_string(),
                    url: url.clone(),
                    timestamp: Local::now(),
                    visited_count: 1,
                    position: 0,
                };
                controller
                    .history
                    .lock()
                    .unwrap()
                    .add(h.clone())
                    .expect("Could not add to history");
                let menu = app
                    .menubar()
                    .find_subtree("History")
                    .expect("history menu missing");
                // Add 3 for the two first menuitems + separator
                if menu.len() > HISTORY_LEN + 3 {
                    menu.remove(menu.len() - 1);
                }
                menu.insert_leaf(3, h.title, move |app| {
                    app.user_data::<Controller>()
                        .expect("controller missing")
                        .open_url(url.clone(), true, 0);
                });
            }))
            .unwrap();
    }

    /// Purges the entire history
    /// TODO: Add option to clear only parts of the history
    pub fn clear_history(&mut self) {
        // Purge file
        self.history
            .lock()
            .unwrap()
            .clear()
            .expect("Could not clear history");
        // empty history menu
        self.sender
            .send(Box::new(|app| {
                let menu = app
                    .menubar()
                    .find_subtree("History")
                    .expect("history menu missing");
                // remove everything but the first three elements
                while menu.len() > 3 {
                    menu.remove(3);
                }
            }))
            .unwrap();
    }

    /// Navigates to the previous page in history
    pub fn navigate_back(&mut self) {
        let mut guard = self.history.lock().unwrap();
        let history = guard.back();
        if let Some(h) = history {
            drop(guard);
            info!("NAVIGATE_BACK to index {}", h.position);
            self.open_url(h.url, false, h.position);
        }
    }

    fn open_command(&mut self, command: &str, url: Url) -> Result<(), Box<dyn Error>> {
        // Opens an image in an external application - if defined in settings
        let command = SETTINGS.read().unwrap().get_str(command)?;
        if !command.is_empty() {
            if let Err(err) = Command::new(&command).arg(url.to_string()).spawn() {
                self.set_message(&format!("Command failed: {}: {}", err, command));
            }
        } else {
            self.set_message(&format!(
                "No command for opening {} defined.",
                url
            ));
        }
        Ok(())
    }

    /// Saves the current text file to disk
    fn save_textfile(&mut self, filename: String) {
        let content = self.content.lock().unwrap().clone();
        info!("Save textfile: {}", filename);
        // Create a path to the desired file
        let path = Path::new(filename.as_str());

        let open = OpenOptions::new()
            .write(true)
            // make sure to not clobber downloaded files
            .create_new(true)
            .open(&path);
        match open {
            Ok(mut file) => {
                if let Err(why) = file.write_all(content.as_bytes()) {
                    self.set_message(&format!("Couldn't open {}: {}", path.display(), why));
                }
            }
            Err(err) => self.set_message(&format!(
                "Unable to open file '{}': {}",
                path.display(),
                err
            )),
        }
    }

    fn save_gemini(&mut self, filename: String) {
        let gemini_content = self.content.lock().unwrap().clone();
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

        let open = OpenOptions::new()
            .write(true)
            // make sure to not clobber downloaded files
            .create_new(true)
            .open(&path);

        match open {
            Ok(mut file) => {
                // Read the file contents into a string, returns `io::Result<usize>`
                for l in lines {
                    if let Err(why) = file.write_all(format!("{}\n", l).as_bytes()) {
                        self.set_message(&format!("Couldn't write {}: {}", path.display(), why));
                        return;
                    }
                }
            }
            Err(err) => {
                self.set_message(&format!(
                    "Unable to open file '{}': {}",
                    path.display(),
                    err
                ));
            }
        }
    }

    /// Save the current gophermap to disk
    fn save_gophermap(&mut self, filename: String) {
        let content = self.content.lock().unwrap().clone();
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
        // FIXME: use url_tools::download_filename_from_url
        let download_path = SETTINGS
            .read()
            .unwrap()
            .get_str("download_path")
            .unwrap_or_default();

        let path = Path::new(download_path.as_str()).join(filename.as_str());
        let display = path.display();

        let open = OpenOptions::new()
            .write(true)
            // make sure to not clobber downloaded files
            .create_new(true)
            .open(&path);

        match open {
            Ok(mut file) => {
                // Read the file contents into a string, returns `io::Result<usize>`
                for l in txtlines {
                    if let Err(why) = file.write_all(format!("{}\n", l).as_bytes()) {
                        self.set_message(&format!("Couldn't open {}: {}", display, why));
                        return;
                    }
                }
            }
            Err(err) => self.set_message(&format!(
                "Unable to open file '{}': {}",
                path.display(),
                err
            )),
        }
    }

    /// Sets message for statusbar
    pub fn set_message(&self, msg: &str) {
        let mut message = self.message.write().unwrap();
        message.clear();
        message.push_str(msg);
        self.sender
            .send(Box::new(move |app| {
                app.clear(); // trigger a refresh
            }))
            .unwrap();
    }

    pub fn get_selected_item_index(app: &mut Cursive) -> usize {
        if let Some(content) = app.find_name::<SelectView<GopherMapEntry>>("content") {
            content.selected_id()
        } else if let Some(content) = app.find_name::<SelectView<Option<Url>>>("gemini_content") {
            content.selected_id()
        } else {
            unreachable!("view content and gemini_content missing");
        }
        .unwrap_or(0)
    }

    pub fn add_bookmark_action(&mut self, url: Url, title: String, tags: String) {
        let tags = tags.as_str().split_whitespace().map(String::from).collect();
        let b = Bookmark { title, url, tags };

        let mut bookmarks = self.bookmarks.lock().unwrap();

        let index = bookmarks.insert(b.clone());

        // add to bookmark menu
        self.sender
            .send(Box::new(move |app| {
                let url = b.url.clone();
                let menu = app
                    .menubar()
                    .find_subtree("Bookmarks")
                    .expect("bookmarks menu missing");
                if let Some(i) = index {
                    // replace element
                    // add 3 to account for "Edit..." etc.
                    menu.remove(i + 3);
                    menu.insert_leaf(i + 3, &b.title, move |app| {
                        app.user_data::<Controller>()
                            .expect("controller missing")
                            .open_url(url.clone(), true, 0);
                    });
                } else {
                    // add new entry to end
                    menu.add_leaf(&b.title, move |app| {
                        app.user_data::<Controller>()
                            .expect("controller missing")
                            .open_url(url.clone(), true, 0);
                    });
                }
            }))
            .unwrap();
    }

    pub fn remove_bookmark_action(app: &mut Cursive, b: Bookmark) {
        let mut guard = app
            .user_data::<Controller>()
            .expect("controller missing")
            .bookmarks
            .lock()
            .unwrap();
        guard.remove(&b.url);
        let bookmarks = guard.entries.clone();
        drop(guard);

        // redraw bookmark menu
        let menutree = app
            .menubar()
            .find_subtree("Bookmarks")
            .expect("bookmarks menu missing");
        menutree.clear();
        // re-add all bookmark entries
        // respecting the order so add_bookmark_action works correctly
        for entry in bookmarks.iter().rev() {
            let url = entry.url.clone();
            menutree.insert_leaf(3, &b.title, move |app| {
                app.user_data::<Controller>()
                    .expect("controller missing")
                    .open_url(url.clone(), true, 0);
            });
        }
    }

    pub fn open_url_action(app: &mut Cursive, url: &str) {
        let controller = app.user_data::<Controller>().expect("controller missing");
        match Url::parse(url) {
            Ok(url) => controller.open_url(url, true, 0),
            Err(e) => controller.set_message(&format!("invalid URL: {}", e)),
        }
    }

    pub fn save_as_action(app: &mut Cursive, path: &str) {
        if !path.is_empty() {
            app.pop_layer();

            let path = path.to_string();
            let controller = app.user_data::<Controller>().expect("controller missing");
            controller.set_message(&format!("saving page as '{}'.", path));

            let current_url = controller.current_url.lock().unwrap().clone();

            match current_url.scheme() {
                "gopher" => {
                    let item_type = ItemType::from_url(&current_url);
                    match item_type {
                        ItemType::Dir => controller.save_gophermap(path),
                        ItemType::File => controller.save_textfile(path),
                        _ => controller.set_message("cannot save this kind of page"),
                    }
                }
                "about" | "gemini" => controller.save_gemini(path),
                other => controller
                    .set_message(&format!("failed to save page: unknown scheme {}", other)),
            }
        } else {
            // do not pop the save dialog so user can make corrections
            app.add_layer(Dialog::info("No filename given!"))
        }
    }

    pub fn certificate_changed_action(app: &mut Cursive, url: &Url, cert_fingerprint: String) {
        let controller = app.user_data::<Controller>().expect("controller missing");
        controller
            .certificates
            .lock()
            .expect("could not lock certificate store")
            .insert(url, cert_fingerprint);
    }
}
