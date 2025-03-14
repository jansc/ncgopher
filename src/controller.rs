use crate::bookmarks::{Bookmark, Bookmarks};
use crate::certificates::Certificates;
use crate::clientcertificates::{ClientCertificate, ClientCertificates};
use crate::gemini::GeminiType;
use crate::gophermap::{GopherMapEntry, ItemType};
use crate::history::{History, HistoryEntry};
use crate::ui::layout::Layout;
use crate::ui::setup::move_to_next_item;
use crate::url_tools::{download_filename_from_url, human_readable_url, normalize_domain};
use crate::SETTINGS;
use base64::engine::general_purpose;
use base64::Engine;
use cursive::{
    theme::ColorStyle,
    utils::{lines::simple::LinesIterator, markup::StyledString},
    view::{Nameable, Resizable},
    views::{Dialog, EditView, NamedView, ResizedView, ScrollView, SelectView},
    Cursive, CursiveRunnable,
};
use linkify::{LinkFinder, LinkKind};
use mime::Mime;
use rcgen::{date_time_ymd, CertificateParams, DistinguishedName, DnType, KeyPair};
use rustls::crypto::{ring as provider, CryptoProvider};
use rustls::pki_types::{CertificateDer, PrivateKeyDer};
use rustls_pemfile::{read_one, Item};
use std::convert::TryInto;
use std::error::Error;
use std::fs::OpenOptions;
use std::io::{BufRead, BufReader, BufWriter, Read, Write};
use std::iter;
use std::net::TcpStream;
use std::path::Path;
use std::process::Command;
use std::rc::Rc;
use std::sync::{Arc, Mutex, RwLock};
use std::thread;
use stringreader::StringReader;
use time::format_description::well_known::Rfc3339;
use time::{Date, OffsetDateTime};
use url::{Position, Url};
use urlencoding::decode_binary;
use x509_parser::prelude::{FromDer, X509Certificate};

#[derive(Clone, Debug)]
pub enum Direction {
    Next,
    Previous,
}

const HISTORY_LEN: usize = 10;

mod danger {
    use rustls::client::danger::HandshakeSignatureValid;
    use rustls::client::danger::ServerCertVerified;
    use rustls::client::danger::ServerCertVerifier;
    use rustls::crypto::{verify_tls12_signature, verify_tls13_signature, CryptoProvider};
    use rustls::pki_types::CertificateDer;
    use rustls::pki_types::ServerName;
    use rustls::pki_types::UnixTime;
    use rustls::DigitallySignedStruct;
    use rustls::SignatureScheme;

    #[derive(Debug)]
    pub struct NoCertificateVerification(CryptoProvider);

    impl NoCertificateVerification {
        pub fn new(provider: CryptoProvider) -> Self {
            Self(provider)
        }
    }

    impl ServerCertVerifier for NoCertificateVerification {
        fn verify_server_cert(
            &self,
            _end_entity: &CertificateDer<'_>,
            _intermediates: &[CertificateDer<'_>],
            _server_name: &ServerName<'_>,
            _ocsp_response: &[u8],
            _now: UnixTime,
        ) -> Result<ServerCertVerified, rustls::Error> {
            Ok(ServerCertVerified::assertion())
        }

        fn verify_tls12_signature(
            &self,
            message: &[u8],
            cert: &CertificateDer<'_>,
            dss: &DigitallySignedStruct,
        ) -> Result<HandshakeSignatureValid, rustls::Error> {
            verify_tls12_signature(
                message,
                cert,
                dss,
                &self.0.signature_verification_algorithms,
            )
            //    Ok(HandshakeSignatureValid::assertion())
        }

        fn verify_tls13_signature(
            &self,
            message: &[u8],
            cert: &CertificateDer<'_>,
            dss: &DigitallySignedStruct,
        ) -> Result<HandshakeSignatureValid, rustls::Error> {
            verify_tls13_signature(
                message,
                cert,
                dss,
                &self.0.signature_verification_algorithms,
            )
            //Ok(HandshakeSignatureValid::assertion())
        }

        fn supported_verify_schemes(&self) -> Vec<SignatureScheme> {
            self.0.signature_verification_algorithms.supported_schemes()
        }
    }
}

macro_rules! client_msg {
    ($sender:ident, $($arg:tt)+) => {
        $sender
            .send(Box::new(move |app| {
                let controller = app.user_data::<Controller>().expect("controller missing");
                controller.set_message(&format!($($arg)+));
            }))
            .unwrap();
    };
}

type SenderCursive = crossbeam_channel::Sender<Box<dyn FnOnce(&mut Cursive) + 'static + Send>>;

#[derive(Clone)]
pub struct Controller {
    sender: SenderCursive,
    /// The browsing history
    pub(crate) history: Rc<Mutex<History>>,
    /// Bookmarks
    pub(crate) bookmarks: Arc<Mutex<Bookmarks>>,
    /// ClientCertificates (gemini)
    pub(crate) client_certificates: Arc<Mutex<ClientCertificates>>,
    /// Known hosts for gemini TOFU
    certificates: Arc<Mutex<Certificates>>,
    /// Current textual content
    content: Arc<Mutex<String>>,
    /// Current URL
    pub current_url: Arc<Mutex<Url>>,
    /// When the user triggers several requests, only the last request
    /// will be displayed, the other will be canceled.
    last_request_id: Arc<Mutex<i64>>,
    /// Number of redirects in gemini protocol
    redirect_count: Arc<Mutex<i32>>,
    /// Message shown in statusbar
    message: Arc<RwLock<String>>,
    // Current search string
    current_search: String,
    // Current search results
    pub current_search_results: Vec<usize>,
}

impl Controller {
    /// Create a new controller (created in main.rs)
    pub fn setup(app: &mut CursiveRunnable, url: Url) -> Result<(), Box<dyn Error>> {
        crate::ui::setup::setup(app);

        let mut controller = Controller {
            sender: app.cb_sink().clone(),
            history: Rc::new(Mutex::new(History::new()?)),
            bookmarks: Arc::new(Mutex::new(Bookmarks::new())),
            client_certificates: Arc::new(Mutex::new(ClientCertificates::new())),
            certificates: Arc::new(Mutex::new(Certificates::new())),
            content: Arc::new(Mutex::new(String::new())),
            current_url: Arc::new(Mutex::new(Url::parse("about:blank").unwrap())),
            last_request_id: Arc::new(Mutex::new(0)),
            redirect_count: Arc::new(Mutex::new(0)),
            message: app
                .find_name::<crate::ui::statusbar::StatusBar>("statusbar")
                .unwrap()
                .get_message(),
            current_search: String::new(),
            current_search_results: Vec::new(),
        };

        let mut entries = controller
            .history
            .lock()
            .unwrap()
            .get_latest_history(HISTORY_LEN)
            .expect("Could not get latest history");
        entries.reverse();
        crate::ui::setup::setup_history_menu(app, &entries);

        let mut entries = controller.bookmarks.lock().unwrap().get_bookmarks();
        entries.reverse();
        crate::ui::setup::setup_bookmark_menu(app, &entries);

        // open initial page
        controller.open_url(url, true, 0);

        app.set_user_data(controller);

        info!("Controller::new() done");

        Ok(())
    }

    fn get_tls_client_config(
        client_cert: &Option<CertificateDer>,
        client_key_pem: &Option<PrivateKeyDer>,
    ) -> rustls::ClientConfig {
        let suites = provider::DEFAULT_CIPHER_SUITES.to_vec();
        let versions = rustls::DEFAULT_VERSIONS.to_vec();
        let config = rustls::ClientConfig::builder_with_provider(
            CryptoProvider {
                cipher_suites: suites,
                ..provider::default_provider()
            }
            .into(),
        )
        .with_protocol_versions(&versions)
        .expect("inconsistent cipher-suite/versions selected");

        let builder = config
            .dangerous()
            .with_custom_certificate_verifier(Arc::new(danger::NoCertificateVerification::new(
                provider::default_provider(),
            )));
        let config = if client_cert.is_some() && client_key_pem.is_some() {
            let client_cert = client_cert.as_ref().unwrap().clone().into_owned();
            builder
                //.with_root_certificates(RootCertStore::empty())
                .with_client_auth_cert(
                    vec![client_cert],
                    client_key_pem.as_ref().unwrap().clone_key(),
                )
                .unwrap()
        } else {
            builder.with_no_client_auth()
        };
        config
    }

    pub fn fetch_gemini_url(&self, mut url: Url, index: usize) {
        if !SETTINGS.read().unwrap().config.disable_history {
            trace!("Controller::fetch_gemini_url({})", url);
        };

        let request_id = {
            let mut guard = self.last_request_id.lock().unwrap();
            *guard += 1;
            *guard
        };
        let request_id_ref = self.last_request_id.clone();
        let redirect_count = self.redirect_count.clone();

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
        let sender = self.sender.clone();

        // Check if a client certificate exists for this host.
        //let mut identity: Option<Identity> = None;
        let mut client_cert_fingerprint: Option<String> = None;

        let mut client_cert: Option<CertificateDer<'static>> = None;
        let mut client_key_pem: Option<PrivateKeyDer<'static>> = None;
        if !SETTINGS.read().unwrap().config.disable_identities {
            // Based on 'url' generate a list of URLs like so:
            // url = gemini://host/a/b/c?foo=bar =>
            // [gemini://host/a/b/c, gemini://host/a/b, gemini://host/a, gemini://host/, gemini://host]
            let mut u = Url::parse(&url[..Position::AfterPath]).unwrap();

            let mut urls: Vec<Url> = vec![u.clone()];

            while u.path() != "" {
                if u.path() == "/" {
                    u.set_path("");
                } else if let Ok(mut path_segments) =
                    u.path_segments_mut().map_err(|_| "cannot be base")
                {
                    path_segments.pop();
                } else {
                    break;
                }
                urls.push(u.clone());
            }
            let mut client_certificates = self.client_certificates.lock().unwrap();
            urls.into_iter().find_map(|url| {
                info!("Checking URL for client certificate match {}", url.as_str());
                if let Some(fingerprint) =
                    client_certificates.get_client_certificate_fingerprint(&url)
                {
                    info!(
                        "Found certificate for URL {} with fingerprint {}",
                        url.as_str(),
                        fingerprint
                    );
                    client_cert_fingerprint = Some(fingerprint.clone());
                    let key_pem = client_certificates.get_cert_by_fingerprint(&fingerprint);
                    if let Some(key_pem) = key_pem {
                        let streader = StringReader::new(key_pem.as_str());
                        let mut bufreader = BufReader::new(streader);
                        for item in iter::from_fn(|| read_one(&mut bufreader).transpose()) {
                            match item.unwrap() {
                                Item::X509Certificate(cert) => {
                                    info!("certificate {:?}", cert);
                                    client_cert = Some(cert);
                                }
                                //Item::RSAKey(key) => println!("rsa pkcs1 key {:?}", key),
                                //Item::PKCS8Key(key) => println!("pkcs8 key {:?}", key),
                                //Item::ECKey(key) => println!("sec1 ec key {:?}", key),
                                _ => info!("Client cert not found"),
                            }
                        }
                    }
                    let private_key_pem =
                        client_certificates.get_private_key_by_fingerprint(&fingerprint);

                    if let Some(pk_pem) = private_key_pem {
                        let reader = StringReader::new(pk_pem.as_str());
                        let mut bufreader = BufReader::new(reader);
                        for item in iter::from_fn(|| read_one(&mut bufreader).transpose()) {
                            match item.unwrap() {
                                Item::Pkcs1Key(key) => {
                                    info!("pkcs1 key {:?}", key);
                                    client_key_pem = Some(key.into())
                                }
                                Item::Pkcs8Key(key) => {
                                    info!("pkcs8 key {:?}", key);
                                    client_key_pem = Some(key.into())
                                }
                                _ => {
                                    info!("unhandled item");
                                }
                            }
                        }
                    }
                    Some(url)
                } else {
                    None
                }
            });
            drop(client_certificates);
        }

        let config = Controller::get_tls_client_config(&client_cert, &client_key_pem);
        thread::spawn(move || {
            let mut buf = String::new();
            let server_name = host.try_into().unwrap();
            let mut conn = rustls::ClientConnection::new(Arc::new(config), server_name).unwrap();

            let mut stream = match TcpStream::connect(server_details) {
                Ok(stream) => stream,
                Err(err) => {
                    client_msg!(sender, "Could not connect to server: {}", err);
                    return;
                }
            };

            let mut tls = rustls::Stream::new(&mut conn, &mut stream);
            let mut cert_opt: Option<&CertificateDer> = None;

            if !SETTINGS.read().unwrap().config.disable_history {
                info!("Writing url '{}'", url.as_str());
            }

            if let Err(err) = tls.write_all(format!("{}\r\n", url).as_bytes()) {
                // Something went wrong, could not write write request URL
                warn!(
                    "Could not write request URL for address {}: {:?}",
                    server_details, err
                );
                client_msg!(sender, "Could not write request address to server.");
                return;
            }

            if let Some(peer_certificates) = tls.conn.peer_certificates() {
                if let Some(cert) = peer_certificates.first() {
                    // Found peer certificate
                    cert_opt = Some(cert);
                }
            } else {
                // Something went wrong, could not get peer certificates
                warn!("Could not get peer certificates for {}", server_details);
                client_msg!(sender, "Could not get peer certificate.");
                return;
            };

            info!("Connected with TLS");

            // check certificate
            if let Some(cert) = cert_opt {
                // TOFU: Check if we already have a certificate fingerprint for a given host
                let hash = ring::digest::digest(&ring::digest::SHA256, cert);
                let cert_fingerprint = general_purpose::STANDARD.encode(hash);
                info!("Peer certificate: {:?}", &cert_fingerprint);

                match fingerprint {
                    Some(f) => {
                        if f != cert_fingerprint {
                            sender
                                .send(Box::new(move |app| {
                                    // Invalid certificate, notify user
                                    let controller =
                                        app.user_data::<Controller>().expect("controller missing");
                                    controller.set_message(&format!(
                                        "Certificate fingerprint DOES NOT match for {}",
                                        url
                                    ));
                                    crate::ui::dialogs::certificate_changed(
                                        app,
                                        url,
                                        cert_fingerprint,
                                    );
                                }))
                                .unwrap();
                            return;
                        } else {
                            let targeturl = url.clone();
                            client_msg!(
                                sender,
                                "Certificate fingerprint matches for {}",
                                targeturl
                            );
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

                if let Ok((_, cert)) = X509Certificate::from_der(cert) {
                    // Check certificate expiration date
                    info!("Successfully parsed certificate");
                    match cert.tbs_certificate.validity.time_to_expiration() {
                        Some(duration) => {
                            let now: OffsetDateTime = OffsetDateTime::now_utc();
                            let expires = now + duration;
                            let expires: OffsetDateTime = expires;
                            info!("Certificate expires {}", expires.format(&Rfc3339).unwrap());
                            info!("Certificate valid {:?}", duration);
                        }
                        None => {
                            client_msg!(sender, "Server certificate expired.");
                        }
                    }
                }
            }

            // Handshake done, request URL from gemini server
            let mut bufr = BufReader::new(tls);
            info!("Reading from gemini stream");
            // Read Gemini Header
            match bufr.read_line(&mut buf) {
                Ok(_) => (),
                Err(e) => {
                    client_msg!(sender, "I/O error: {}", e);
                    return;
                }
            }
            let trimmed_buf = buf.trim();
            // "text/gemini; charset=utf-8"
            info!("Got gemini header: {}:  {}", trimmed_buf.len(), trimmed_buf);

            {
                // Abort request, if user triggered a newer request
                let guard = request_id_ref.lock().unwrap();
                if request_id < *guard {
                    return;
                }
            }

            if buf.is_empty() {
                client_msg!(sender, "Could not read from stream");
                return;
            }

            // <META> always starts at the 4th char
            // (it might contain leading whitespace)
            let meta = buf.chars().skip(3).collect::<String>();
            // <META> has a maximum size
            if meta.len() > 1024 {
                client_msg!(sender, "Invalid header from server: <META> too large");
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
                    let char = buf.chars().take(2).collect::<String>();
                    client_msg!(sender, "Unknown status code {}", char);
                } else {
                    // either the second char is not an ASCII digit
                    // or does not exist at all
                    let buf_str = buf.to_string();
                    client_msg!(
                        sender,
                        "Invalid header from server: invalid status code: {}",
                        buf_str
                    );
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
                        let buf_str = buf.to_string();
                        client_msg!(
                            sender,
                            "Invalid header from server: malformed: {}",
                            &buf_str
                        );
                        return false;
                    }
                }
                true
            };

            let status = buf.chars().next();
            // Reset redirect count when the status does not indicate a redirect
            if status != Some('3') {
                let mut guard = redirect_count.lock().unwrap();
                *guard = 0;
            }

            match status {
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
                            client_msg!(sender, "I/O error: {}", err);
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
                            controller.clear_search();
                            controller.set_message(url.as_str());
                            controller.set_gemini_content(url, gemini_type, s, index, client_cert_fingerprint);
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
                                    client_msg!(sender, "{} bytes read", total_written);
                                }
                                sender
                                    .send(Box::new(move |app| {
                                        let controller =
                                            app.user_data::<Controller>().expect("controller missing");
                                        controller.set_message(&format!(
                                            "File downloaded: {} ({} bytes)",
                                            local_filename, total_written
                                        ));
                                        if mime.type_() == "image" {
                                            let path = Path::new(&local_filename);
                                            controller.open_image_from_file(path).ok();
                                        }
                                    }))
                                    .unwrap();
                            }
                            Err(err) => {
                                sender
                                    .send(Box::new(move |app| {
                                        let controller =
                                            app.user_data::<Controller>().expect("controller missing");
                                        controller.set_message(&format!(
                                            "Unable to open file '{}': {}",
                                            local_filename, err
                                        ));
                                    }))
                                    .unwrap();
                            }
                        }
                    }
                }
                Some('3') => {
                    // REDIRECT
                    let redirect_count = {
                        let mut guard = redirect_count.lock().unwrap();
                        *guard += 1;
                        *guard
                    };
                    if redirect_count >= 5 {
                        sender.send(Box::new(move |app|{
                            let controller = app.user_data::<Controller>().expect("controller missing");
                            controller.set_gemini_content(url.clone(), GeminiType::Gemini,
                            format!("# Too many redirects\n\nYou are probably stuck in a redirect loop. \
                                     Here is the next redirected URL if you want to continue manually:\n\n=> {}", url), 0,
                            None);
                            controller.set_message("Detected redirect loop.");
                        })).unwrap();
                        return;
                    }

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
                            sender.send(Box::new(move |app|{
                                let controller = app.user_data::<Controller>().expect("controller missing");
                                controller.open_url(url, true, 0);
                            })).unwrap();
                        }
                        Err(_) => {
                            sender
                                .send(Box::new(move |app| {
                                    let controller =
                                        app.user_data::<Controller>().expect("controller missing");
                                    controller.set_message(&format!(
                                        "invalid redirect url: {}",
                                        meta
                                    ));
                                }))
                                .unwrap();
                        }
                    }
                }
                Some('4') // FAILURE
                | Some('5') // PERMANENT FAILURE
                | Some('6') // CLIENT CERTIFICATE
                => {
                    if check(buf.chars().nth(1)) {
                        if status == Some('6') && buf.chars().nth(1) == Some('0') {
                            if SETTINGS.read().unwrap().config.disable_identities {
                                sender.send(Box::new(move |app|{
                                    app.add_layer(Dialog::info("The server requests a client certificate, but\n\
                                                                identities are globally disabled in the settings."));
                                })).unwrap();
                            } else {
                                sender.send(Box::new(move |app|{
                                    crate::ui::dialogs::choose_client_certificate(app, url);
                                })).unwrap();
                            }
                        } else if status == Some('6') && buf.chars().nth(1) == Some('1') {
                            debug!("TODO: Handle gemini code 61 - certificate not authorized");

                            // FIXME: Rewrite this
                            let header = buf.to_string();
                            sender.send(Box::new(move |app|{
                                let controller = app.user_data::<Controller>().expect("controller missing");
                                // reset content and set current URL for retrying
                                controller.set_gemini_content(url, GeminiType::Text, String::new(), 0, None);
                                controller.set_message(&format!("Gemini error: {}", header));
                            })).unwrap();
                        } else { // FAILURE, PERMANENT FAILURE, etc.
                            let header = buf.to_string();
                            sender.send(Box::new(move |app|{
                                let controller = app.user_data::<Controller>().expect("controller missing");
                                // reset content and set current URL for retrying
                                controller.set_gemini_content(url, GeminiType::Text, String::new(), 0, None);
                                controller.set_message(&format!("Gemini error: {}", header));
                            })).unwrap();
                        }
                    }
                }
                other => {
                    if other.is_some() {
                        client_msg!(sender, "invalid header from server: invalid status code: {}", buf);
                    } else {
                        client_msg!(sender, "invalid header from server: missing status code: {}", buf);
                    };
                }
            }
            info!("finished reading from gemini stream");
        });
    }

    fn fetch_url(&self, url: Url, item_type: ItemType, index: usize) {
        // index is the position in the text (used when navigating back or reloading)
        if !SETTINGS.read().unwrap().config.disable_history {
            trace!("Controller::fetch_url({})", url);
        }

        let request_id = {
            let mut guard = self.last_request_id.lock().unwrap();
            *guard += 1;
            *guard
        };

        let port = url.port().unwrap_or(70);
        let server = url.host_str().expect("no host").to_string();
        let path = url.path();

        // Decode %xx to binary values. Fixes #78
        let binary = decode_binary(path.as_bytes());
        let mut path = String::from_utf8_lossy(&binary).as_ref().to_owned();

        if !SETTINGS.read().unwrap().config.disable_history {
            info!("fetch_url(): About to open URL {}", path);
        }
        if path.len() > 2 {
            // TODO: check x[0] == / and x[1] == itemtype
            path = path[2..].to_string();
        } else {
            path = "".to_string();
        }

        let server_details = format!("{}:{}", server, port);

        let request_id_ref = self.last_request_id.clone();
        let sender = self.sender.clone();

        let config = Controller::get_tls_client_config(&None, &None);
        thread::spawn(move || {
            let mut use_tls = false;
            let mut buf = vec![];
            // TLS-support. If non-standard-port, try to connect with TLS
            if port != 70 {
                let server_name = server.clone().try_into().unwrap();
                let mut conn =
                    rustls::ClientConnection::new(Arc::new(config), server_name).unwrap();

                let stream = TcpStream::connect(server_details.clone());
                if stream.is_ok() {
                    let mut stream = stream.unwrap();
                    match conn.complete_io(&mut stream) {
                        Err(err) => {
                            error!("Could not complete TLS handshake: {:?}", err);
                            use_tls = false;
                        }
                        Ok(_) => {
                            info!("Now connected with tls");
                            use_tls = true;
                        }
                    }
                    let mut tls = rustls::Stream::new(&mut conn, &mut stream);
                    if use_tls {
                        if let Err(err) = tls.write_all(format!("{}\r\n", path).as_bytes()) {
                            // Something went wrong, could not write write request URL
                            use_tls = false;
                            warn!(
                                "Could not write request URL for address {}: {:?}",
                                server_details.clone(),
                                err
                            );
                        }
                    }
                    if use_tls {
                        let mut bufr = BufReader::new(tls);
                        loop {
                            match bufr.read_to_end(&mut buf) {
                                Ok(_) => break,
                                Err(e) => {
                                    client_msg!(sender, "I/O error: {}", e);
                                }
                            };
                        }
                    }
                } else {
                    use_tls = false;
                }
            }
            // TLS connection failed or still on port 70
            if !use_tls {
                match TcpStream::connect(server_details.clone()) {
                    Ok(mut stream) => {
                        write!(stream, "{}\r\n", path).unwrap();
                        loop {
                            match stream.read_to_end(&mut buf) {
                                Ok(_) => break,
                                Err(e) => {
                                    client_msg!(sender, "I/O error: {}", e);
                                }
                            }
                        }
                    }
                    Err(e) => {
                        client_msg!(sender, "Couldn't connect to server: {}", e);
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
                    controller.clear_search();
                    controller.set_gopher_content(item_type, s, index);
                }))
                .unwrap();
        });
    }

    fn fetch_binary_url(&mut self, url: Url, item_type: ItemType, local_filename: String) {
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
        let sender = self.sender.clone();
        let config = Controller::get_tls_client_config(&None, &None);
        thread::spawn(move || {
            // FIXME: Error handling!
            let mut use_tls = false;
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
                        let server_name = server.clone().try_into().unwrap();
                        let mut conn =
                            rustls::ClientConnection::new(Arc::new(config), server_name).unwrap();

                        let stream = TcpStream::connect(server_details.clone());
                        if stream.is_ok() {
                            let mut stream = stream.unwrap();
                            match conn.complete_io(&mut stream) {
                                Err(err) => {
                                    error!("Could not complete TLS handshake: {:?}", err);
                                    use_tls = false;
                                }
                                Ok(_) => {
                                    info!("Now connected with tls");
                                    use_tls = true;
                                }
                            }
                            let mut tls = rustls::Stream::new(&mut conn, &mut stream);
                            if use_tls {
                                if let Err(err) = tls.write_all(format!("{}\r\n", path).as_bytes())
                                {
                                    // Something went wrong, could not write write request URL
                                    use_tls = false;
                                    warn!(
                                        "Could not write request URL for address {}: {:?}",
                                        server_details.clone(),
                                        err
                                    );
                                }
                            }
                            if use_tls {
                                let mut bufr = BufReader::new(tls);
                                loop {
                                    let bytes_read =
                                        bufr.read(&mut buf).expect("Could not read from TCP");
                                    if bytes_read == 0 {
                                        break;
                                    }
                                    let bytes_written = bw
                                        .write(&buf[..bytes_read])
                                        .expect("Could not write to file");
                                    total_written += bytes_written;
                                    client_msg!(sender, "{} bytes written", total_written);
                                }
                            }
                        } else {
                            use_tls = false;
                        }
                    }
                    if !use_tls {
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
                            client_msg!(sender, "{} bytes written", total_written);
                        }
                    }
                    sender
                        .send(Box::new(move |app| {
                            let controller =
                                app.user_data::<Controller>().expect("controller missing");
                            controller.set_message(&format!(
                                "File downloaded: {} ({} bytes)",
                                local_filename, total_written
                            ));
                            if item_type == ItemType::Gif || item_type == ItemType::Image {
                                let path = Path::new(&local_filename);
                                controller.open_image_from_file(path).ok();
                            }
                        }))
                        .unwrap();
                }
                Err(err) => {
                    client_msg!(sender, "Unable to open file: '{}' {}", local_filename, err);
                }
            }
        });
    }

    pub fn open_url(&mut self, url: Url, add_to_history: bool, index: usize) {
        if !SETTINGS.read().unwrap().config.disable_history {
            info!("Open_url: {} position {}", url, index);
        }
        if add_to_history {
            self.add_to_history(url.clone(), index);
        }
        *self.current_url.lock().unwrap() = url.clone();
        match url.scheme() {
            "finger" => self.open_finger_address(url.clone(), index),
            "gopher" => self.open_gopher_address(url.clone(), ItemType::from_url(&url), index),
            "gemini" => self.open_gemini_address(url.clone(), index),
            "about" => self.open_about(url.clone()),
            "http" | "https" => self.open_command("html_command", url.clone()).unwrap(),
            scheme => self.set_message(format!("unknown scheme {}", scheme).as_str()),
        }
    }

    fn fetch_finger_url(&self, url: Url, index: usize) {
        // index is the position in the text (used when navigating back or reloading)
        if !SETTINGS.read().unwrap().config.disable_history {
            trace!("Controller::fetch_finger_url({})", url);
        }

        let request_id = {
            let mut guard = self.last_request_id.lock().unwrap();
            *guard += 1;
            *guard
        };

        let port = url.port().unwrap_or(79);
        let server = url.host_str().expect("no host").to_string();
        let username = <&str>::clone(&url.username());
        let path = match username.is_empty() {
            true => url.path().trim_matches('/').to_string(),
            false => username.to_string(),
        };
        let server_details = format!("{}:{}", server, port);
        let request_id_ref = self.last_request_id.clone();
        let sender = self.sender.clone();

        thread::spawn(move || {
            let mut buf = vec![];
            match TcpStream::connect(server_details.clone()) {
                Ok(mut stream) => {
                    write!(stream, "{}\r\n", path).unwrap();
                    loop {
                        match stream.read_to_end(&mut buf) {
                            Ok(_) => break,
                            Err(e) => {
                                client_msg!(sender, "I/O error: {}", e);
                            }
                        }
                    }
                }
                Err(e) => {
                    client_msg!(sender, "Couldn't connect to server: {}", e);
                    return;
                }
            };

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
                    controller.clear_search();
                    controller.set_finger_content(url, s, index);
                }))
                .unwrap();
        });
    }

    /// Show an internal page from the "about" URL scheme
    /// as defined in RFC 6694.
    fn open_about(&mut self, url: Url) {
        let content = match url.path() {
            "blank" => String::new(),
            "help" => include_str!("about/help.gmi").into(),
            "release-notes" => include_str!("about/release-notes.gmi").into(),
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
        self.set_gemini_content(url, GeminiType::Gemini, content, 0, None);
        self.clear_search();
    }

    pub fn open_gopher_address(&mut self, url: Url, item_type: ItemType, index: usize) {
        self.set_message("Loading ...");
        if item_type.is_download() {
            let filename = download_filename_from_url(&url);
            self.fetch_binary_url(url, item_type, filename);
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
            self.clear_search();
            let human_url = human_readable_url(&self.current_url.lock().unwrap());

            // Issue #210: Note: Lines beginning with periods must be
            // prepended with an extra period to ensure that the
            // transmission is not terminated early. The client should
            // strip extra periods at the beginning of the line.
            let content_without_dots = content
                .lines()
                .map(|line| {
                    if !line.is_empty() && line.starts_with('.') {
                        line[1..].to_string()
                    } else {
                        line[0..].to_string()
                    }
                })
                .collect::<Vec<String>>()
                .join("\n");
            self.set_gemini_content(
                Url::parse(&human_url).unwrap(),
                GeminiType::Text,
                content_without_dots,
                index,
                None,
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
                    .config
                    .textwrap
                    .parse()
                    .unwrap_or(usize::MAX);

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
                                    path.push_str(&name);
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
                    } else if entry.item_type.is_inline() {
                        // Check if current line is text only. If yes, try to find
                        // URL in text and open with appropriate function
                        controller.open_link_in_label(entry.clone().label());
                    }
                });
                view.set_selection(index);
            }))
            .unwrap();
    }

    fn open_link_in_label(&mut self, label: String) {
        self.sender
            .send(Box::new(move |app| {
                let finder = LinkFinder::new();
                let links: Vec<_> = finder.links(&label).collect();
                if links.len() == 1 && links[0].kind() == &LinkKind::Url {
                    let link = &links[0];
                    if let Ok(url) = Url::parse(link.as_str()) {
                        app.user_data::<Controller>()
                            .expect("controller missing")
                            .open_url(url, true, 0);
                    }
                } else if links.len() > 1 {
                    app.add_layer(Dialog::info(
                        "Found several links, not sure which one to open.\nDialog not implemented",
                    ));
                }
            }))
            .unwrap();
    }

    fn open_gemini_address(&mut self, url: Url, index: usize) {
        self.set_message("Loading ...");
        self.fetch_gemini_url(url, index);
    }

    fn open_finger_address(&mut self, url: Url, index: usize) {
        self.set_message("Loading ...");
        self.fetch_finger_url(url, index);
    }

    fn set_gemini_content(
        &mut self,
        url: Url,
        gemini_type: GeminiType,
        content: String,
        index: usize,
        cert_fingerprint: Option<String>,
    ) {
        let mut guard = self.content.lock().unwrap();
        guard.clear();
        guard.push_str(content.as_str());
        drop(guard);

        let mut cert_common_name_label = String::new();
        if let Some(fingerprint) = cert_fingerprint {
            if let Some(cc) = self
                .client_certificates
                .lock()
                .unwrap()
                .get_client_certificate(&fingerprint)
            {
                cert_common_name_label.push_str(format!("[Identity: {}]", cc.common_name).as_str());
            }
        }

        let human_url = human_readable_url(&url);
        // ensure gemini view is focused before setting content
        self.sender
            .send(Box::new(move |app| {
                // set title
                let mut layout = app
                    .find_name::<Layout>("main")
                    .expect("main layout missing");
                layout.set_view("gemini_content");
                layout.set_title(
                    "gemini_content".into(),
                    format!("{} {}", human_url, cert_common_name_label),
                );
                info!("set gemini view");
            }))
            .unwrap();

        self.sender
            .send(Box::new(move |app| {
                let textwrap = SETTINGS
                    .read()
                    .unwrap()
                    .config
                    .textwrap
                    .parse()
                    .unwrap_or(usize::MAX);

                let viewport_width = app.screen_size().x
                // adjust for left margin
                - 8;

                let viewport_width = std::cmp::min(textwrap, viewport_width);

                let mut view = app
                    .find_name::<SelectView<Option<Url>>>("gemini_content")
                    .expect("gemini content view missing");
                view.clear();

                if gemini_type == GeminiType::Text {
                    let content = str::replace(&content, "\t", "        ");
                    view.add_all(
                        LinesIterator::new(&content, viewport_width)
                            .map(|row| (&content[row.start..row.end], None))
                            .collect::<Vec<_>>(),
                    );
                } else {
                    view.add_all(crate::gemini::parse(&content, &url, viewport_width));
                }
                view.set_on_submit(|app, _entry| {
                    let view = app
                        .find_name::<SelectView<Option<Url>>>("gemini_content")
                        .expect("gemini content view missing");
                    if let Some(selected_id) = view.selected_id() {
                        if let Some((label, entry)) = view.get_item(selected_id) {
                            if let Some(url) = entry {
                                app.user_data::<Controller>()
                                    .expect("controller missing")
                                    .open_url(url.clone(), true, 0)
                            } else {
                                let controller =
                                    app.user_data::<Controller>().expect("controller missing");
                                controller.open_link_in_label(label.to_string());
                            }
                        }
                    }
                });
                view.set_selection(index);
            }))
            .unwrap();
    }

    /// Renders finger content
    fn set_finger_content(&mut self, url: Url, content: String, index: usize) {
        let mut guard = self.content.lock().unwrap();
        guard.clear();
        guard.push_str(content.as_str());
        drop(guard);

        self.clear_search();
        self.set_gemini_content(url, GeminiType::Text, content, index, None);
    }

    fn add_to_history(&mut self, url: Url, index: usize) {
        if SETTINGS.read().unwrap().config.disable_history {
            return;
        }
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
                    timestamp: OffsetDateTime::now_local().unwrap_or(OffsetDateTime::now_utc()),
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
                if let Some(idx) = menu.find_position(url.as_ref()) {
                    if idx >= 3 {
                        menu.remove(idx);
                    }
                }
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
        // Opens a URL in an external application - if defined in settings
        let command = match command {
            "html_command" => SETTINGS.read().unwrap().config.html_command.clone(),
            "image_command" => SETTINGS.read().unwrap().config.image_command.clone(),
            "telnet_command" => SETTINGS.read().unwrap().config.telnet_command.clone(),
            _ => panic!("unknown field"),
        };

        if !command.is_empty() {
            if let Err(err) = Command::new(&command).arg(url.to_string()).spawn() {
                self.set_message(&format!("Command failed: {}: {}", err, command));
            }
        } else {
            self.set_message(&format!("No command for opening {} defined.", url));
        }
        Ok(())
    }

    fn open_image_from_file(&mut self, path: &Path) -> Result<(), Box<dyn Error>> {
        let command = SETTINGS.read().unwrap().config.image_command.clone();
        if !command.is_empty() {
            if let Err(err) = Command::new(&command)
                .arg(path.as_os_str().to_str().unwrap())
                .spawn()
            {
                self.set_message(&format!("Command failed: {}: {}", err, command));
            }
        } else {
            self.set_message(&format!(
                "No command for opening {} defined.",
                path.as_os_str().to_str().unwrap()
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
            .open(path);
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
        let download_path = SETTINGS.read().unwrap().config.download_path.clone();

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
        let download_path = SETTINGS.read().unwrap().config.download_path.clone();

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
                // Send a no-op callback to trigger a refresh
                // See cursive issue #244
                app.cb_sink().send(Box::new(|_| {})).unwrap();
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

    pub fn remove_client_certificate_action(app: &mut Cursive, cc: &ClientCertificate) {
        let mut guard = app
            .user_data::<Controller>()
            .expect("controller missing")
            .client_certificates
            .lock()
            .unwrap();
        guard.remove(&cc.fingerprint);
        drop(guard);
    }

    pub fn use_current_site_client_certificate_action(
        app: &mut Cursive,
        cc: ClientCertificate,
    ) -> bool {
        let current_url: Url = app
            .user_data::<Controller>()
            .expect("controller missing")
            .current_url
            .lock()
            .unwrap()
            .clone();
        if current_url.scheme() == "gemini" {
            let mut guard = app
                .user_data::<Controller>()
                .expect("controller missing")
                .client_certificates
                .lock()
                .unwrap();
            guard.use_current_site(&current_url, &cc.fingerprint);
            drop(guard);
            true
        } else {
            false
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

    pub fn create_client_certificate(
        &mut self,
        common_name: String,
        note: String,
        expiration_date: Date,
        specified_url: Option<Url>,
    ) {
        let mut params: CertificateParams = Default::default();
        let now = OffsetDateTime::now_utc().date();
        params.not_before = date_time_ymd(now.year(), now.month().into(), now.day());
        params.not_after = date_time_ymd(
            expiration_date.year(),
            expiration_date.month().into(),
            expiration_date.day(),
        );
        params.distinguished_name = DistinguishedName::new();
        params
            .distinguished_name
            .push(DnType::CommonName, common_name.as_str());
        if let Ok(key_pair) = KeyPair::generate() {
            if let Ok(cert) = params.self_signed(&key_pair) {
                let (cert_pem, private_key) = (cert.pem(), key_pair.serialize_pem());
                // Create fingerprint:
                let der_serialized = cert.der();
                let hash = ring::digest::digest(&ring::digest::SHA256, der_serialized);
                let fingerprint: String = hash
                    .as_ref()
                    .iter()
                    .map(|b| format!("{:02X}", b))
                    .collect::<Vec<String>>()
                    .join(":");

                let client_certificate = ClientCertificate {
                    common_name,
                    note,
                    fingerprint,
                    cert: cert_pem,
                    private_key,
                    expiration_date,
                };
                self.client_certificates
                    .lock()
                    .unwrap()
                    .insert(client_certificate, &specified_url);
            }
        }
    }

    pub fn update_client_certificate(&mut self, cc: &ClientCertificate, urls: Vec<Url>) {
        self.client_certificates.lock().unwrap().update(cc, urls);
    }

    pub fn search(&mut self, search_str: String) {
        info!("Searching for {}", search_str);
        self.current_search = search_str.clone();
        let sender = self.sender.clone();
        sender
            .send(Box::new(move |app| {
                let mut hits = Vec::new();
                if let Some(mut content) = app.find_name::<SelectView<GopherMapEntry>>("content") {
                    for (index, listitem) in content.try_iter_mut().enumerate() {
                        let (label, _item) = listitem; //(&mut SpannedString<Style>, &mut GopherMapEntry)
                        let label_source = label.source();
                        if !search_str.is_empty() && label_source.contains(&search_str) {
                            hits.push(index);
                            let split = label_source.split(&search_str);
                            let mut l = StyledString::new();

                            let vec: Vec<&str> = split.collect();
                            for (pos, part) in vec.iter().enumerate() {
                                l.append(*part);
                                if pos != vec.len() - 1 {
                                    //l.append_styled(&search_str, ColorStyle::new(Color::Dark(BaseColor::Red), ColorType::Palette(PaletteColor::Highlight)));
                                    l.append_styled(&search_str, ColorStyle::highlight());
                                }
                            }
                            *label = l.clone();
                        } else {
                            // This will remove previous search results
                            let mut l = StyledString::new();
                            l.append(label_source);
                            *label = l.clone();
                        }
                    }
                    let scroll_view = app.find_name::<ScrollView<ResizedView<NamedView<SelectView<GopherMapEntry>>>>>(
                        "content_scroll",
                        ).expect("gopher scroll view missing");
                    move_to_next_item(content, scroll_view, Direction::Next, hits.clone());
                } else if let Some(mut content) = app.find_name::<SelectView<Option<Url>>>("gemini_content") {
                    info!("Found gemini content!!!!");
                    for (index, listitem) in content.try_iter_mut().enumerate() {
                        let (label, _item) = listitem; //(&mut SpannedString<Style>, &mut GopherMapEntry)
                        let label_source = label.source();
                        if !search_str.is_empty() && label_source.contains(&search_str) {
                            hits.push(index);
                            let split = label_source.split(&search_str);
                            let mut l = StyledString::new();

                            let vec: Vec<&str> = split.collect();
                            for (pos, part) in vec.iter().enumerate() {
                                l.append(*part);
                                if pos != vec.len() - 1 {
                                    l.append_styled(&search_str, ColorStyle::highlight());
                                }
                            }
                            *label = l.clone();
                        } else {
                            // This will remove previous search results
                            let mut l = StyledString::new();
                            l.append(label_source);
                            *label = l.clone();
                        }
                    }
                    let scroll_view = app.find_name::<ScrollView<ResizedView<NamedView<SelectView<Option<Url>>>>>>(
                        "gemini_content_scroll",
                        ).expect("gemini scroll view missing");
                    move_to_next_item(content, scroll_view, Direction::Next, hits.clone());
                } else {
                    unreachable!("view content and gemini_content missing");
                }
                info!("Found hits: {:?}", hits);
                app.user_data::<Controller>()
                    .expect("controller missing")
                    .set_search_hits(hits.clone());
            })).unwrap();
    }

    pub fn set_search_hits(&mut self, hits: Vec<usize>) {
        self.current_search_results = hits;
    }

    pub fn clear_search(&mut self) {
        self.current_search_results.clear();
    }
}
