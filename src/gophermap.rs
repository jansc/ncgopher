use regex::Regex;
use url::Url;

/// An menu item in a directory of Gopher resources.
#[derive(Clone, Debug)]
pub struct GopherMapEntry {
    /// The type of the resource
    pub item_type: ItemType,
    /// String to display to the user.
    pub name: String,
    /// Path or identifier used for requesting this resource.
    pub selector: String,
    /// The hostname of the server hosting this resource.
    pub host: String,
    /// The TCP port of the server hosting this resource.
    pub port: u16,
    /// The combined URL of host, port and selector
    pub url: Url,
}

impl GopherMapEntry {
    /// Parses a raw string into a GopherMapEntry
    pub fn parse(line: String) -> Result<Self, &'static str> {
        let l = line.split_terminator('\t').collect::<Vec<_>>();
        // Sometimes there are empty lines in a gophermap.
        // Ignore these.
        if l.is_empty() {
            return Ok(GopherMapEntry {
                item_type: ItemType::Inline,
                name: "".to_string(),
                selector: "/".to_string(),
                host: "about:blank".to_string(),
                port: 70,
                url: Url::parse("about:blank").unwrap(),
            });
        }
        if l.is_empty() {
            // Happens e.g. if a text file is parsed as a gophermap
            return Err("Invalid gophermap entry (2)");
        }
        if l[0].is_empty() {
            return Err("Invalid gophermap entry, no item type");
        }
        let ch = l[0].chars().next().unwrap();
        let item_type = ItemType::decode(ch);

        let mut name = l[0][ch.len_utf8()..].to_string();

        // Remove ANSI sequences. baud.baby, I'm looking at you
        let ansi_sequences = Regex::new(r"(\x9B|\x1B\[)[0-?]*[ -/]*[@-~]").unwrap();
        name = ansi_sequences.replace_all(name.as_str(), "").to_string();

        let mut url = Url::parse("gopher://example.com").unwrap();
        let mut selector = String::from("");
        let mut host = String::from("");
        let mut port = 70;
        let mut path;
        if item_type == ItemType::Inline && l.len() == 1 {
            // Add support for item type inline without selector and host
            return Ok(GopherMapEntry {
                item_type,
                name,
                selector,
                host,
                port,
                url,
            })
        } else {
            if l.len() <= 3 {
                // Happens e.g. if a text file is parsed as a gophermap
                return Err("Invalid gophermap entry (4)");
            }
            selector = l[1].to_string();
            host = l[2].to_string();
            // Parse port, ignore invalid values
            port = l[3].parse().unwrap_or(70);
            path = selector.clone();
            path.insert(0, ch);
        }

        if item_type == ItemType::Telnet {
            // Telnet URLs have no selector
            url.set_scheme("telnet").unwrap();
            if !host.is_empty() {
                url.set_host(Some(host.as_str())).unwrap();
            }
            url.set_port(Some(port)).unwrap();
        } else if item_type == ItemType::Html {
            if path.starts_with("hURL:") {
                let mut html_url = path;
                html_url.replace_range(..5, "");
                match Url::parse(html_url.as_str()) {
                    Ok(u) => url = u,
                    Err(e) => {
                        warn!("Could not parse url {}: {}", e, html_url);
                    }
                }
            }
        } else {
            if !host.is_empty() {
                if let Err(e) = url.set_host(Some(host.as_str())) {
                    warn!("Could not parse host {}: {}", host.as_str(), e);
                    return Err("Invalid host");
                }
            }
            url.set_port(Some(port)).unwrap();
            url.set_path(path.as_str());
        }
        Ok(GopherMapEntry {
            item_type,
            name,
            selector,
            host,
            port,
            url,
        })
    }

    pub fn label(self) -> String {
        self.name
    }
}

/// The type of a resource in a Gopher directory.
///
/// For more details, see: https://tools.ietf.org/html/rfc1436
#[derive(Debug, Eq, PartialEq, Hash, Copy, Clone)]
pub enum ItemType {
    /// Item is a file
    File,
    /// Item is a directory
    Dir,
    /// Item is a CSO phone-book server
    CsoServer,
    /// Error
    Error,
    /// Item is a BinHexed Macintosh file.
    BinHex,
    /// Item is DOS binary archive of some sort.
    ///
    /// Client must read until the TCP connection closes.  Beware.
    Dos,
    /// Item is a UNIX uuencoded file.
    Uuencoded,
    /// Item is an Index-Search server.
    IndexServer,
    /// Item points to a text-based telnet session.
    Telnet,
    /// Item is a binary file! Client must read until the TCP connection closes.  Beware
    Binary,
    /// Item is a redundant server
    RedundantServer,
    /// Item points to a text-based tn3270 session.
    Tn3270,
    /// Item is a GIF format graphics file.
    Gif,
    /// Item is some kind of image file.  Client decides how to display.
    Image,
    /// Item is a HTML link
    Html,
    /// Item is a document
    Document,
    /// Item is a video file
    Video,
    /// Item is MIME encoded file
    Mime,
    /// Item is a calendar file (ical?)
    Calendar,
    /// Item is a sound file
    Sound,
    /// Item is inline text or info line
    Inline,
    /// Item is a non-standard type
    Other(char),
}

impl ItemType {
    pub fn decode(b: char) -> Self {
        match b {
            '0' => ItemType::File,
            '1' => ItemType::Dir,
            '2' => ItemType::CsoServer,
            '3' => ItemType::Error,
            '4' => ItemType::BinHex,
            '5' => ItemType::Dos,
            '6' => ItemType::Uuencoded,
            '7' => ItemType::IndexServer,
            '8' => ItemType::Telnet,
            '9' => ItemType::Binary,
            '+' => ItemType::RedundantServer,
            'T' => ItemType::Tn3270,
            'g' => ItemType::Gif,
            'I' => ItemType::Image,
            'h' => ItemType::Html,
            'd' => ItemType::Document,
            ';' => ItemType::Video,
            'M' => ItemType::Mime,
            'c' => ItemType::Calendar,
            's' => ItemType::Sound,
            'i' => ItemType::Inline,
            ch => ItemType::Other(ch),
        }
    }

    pub fn as_str(item_type: ItemType) -> String {
        match item_type {
            ItemType::File => "[TXT]",
            ItemType::Dir => "[MAP]",
            ItemType::CsoServer => "[CSO]",
            ItemType::Error => "[ERR]",
            ItemType::BinHex => "[BIN]",
            ItemType::Dos => "[DOS]",
            ItemType::Uuencoded => "[UU] ",
            ItemType::IndexServer => "[QRY]",
            ItemType::Telnet => "[TEL]",
            ItemType::Binary => "[BIN]",
            ItemType::RedundantServer => "[RED]",
            ItemType::Tn3270 => "[TRM]",
            ItemType::Gif => "[GIF]",
            ItemType::Image => "[IMG]",
            ItemType::Html => "[HTM]",
            ItemType::Document => "[DOC]",
            ItemType::Video => "[VID]",
            ItemType::Mime => "[MME]",
            ItemType::Calendar => "[CAL]",
            ItemType::Sound => "[SND]",
            ItemType::Inline => "     ",
            ItemType::Other(_ch) => "[???]",
        }
        .to_string()
    }

    pub fn is_download(self) -> bool {
        matches!(
            self,
            ItemType::BinHex
                | ItemType::Dos
                | ItemType::Uuencoded
                | ItemType::Binary
                | ItemType::Gif
                | ItemType::Image
                | ItemType::Document
                | ItemType::Video
                | ItemType::Mime
                | ItemType::Calendar
                | ItemType::Sound
        )
    }

    pub fn is_text(self) -> bool {
        matches!(self, ItemType::File)
    }

    pub fn is_dir(self) -> bool {
        matches!(self, ItemType::Dir)
    }

    pub fn is_query(self) -> bool {
        matches!(self, ItemType::IndexServer)
    }

    pub fn is_inline(self) -> bool {
        matches!(self, ItemType::Inline)
    }

    pub fn is_image(self) -> bool {
        matches!(self, ItemType::Gif | ItemType::Image)
    }

    pub fn is_telnet(self) -> bool {
        matches!(self, ItemType::Telnet)
    }

    pub fn is_html(self) -> bool {
        matches!(self, ItemType::Html)
    }

    /// Returns the ItemType of an url. Defaults to gophermap (ItemType::Dir 1)
    pub fn from_url(url: &Url) -> ItemType {
        let path = url.path();
        let mut item_type = ItemType::Dir;
        let mut chars = path.chars();
        if path.chars().count() > 2 && chars.next().unwrap() == '/' {
            item_type = ItemType::decode(chars.next().unwrap());
        }
        item_type
    }
}
