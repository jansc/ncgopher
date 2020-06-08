use url::Url;

/// A list of Gopher resources.
/*
pub struct GopherMap {
    pub entities: Vec<GopherMapEntry>,
}
*/

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
        let l: Vec<&str> = line.split_terminator('\t').collect();
        // Sometimes there are empty lines in a gophermap.
        // Ignore these.
        if l.is_empty() {
            return Ok(GopherMapEntry {
                item_type: ItemType::Inline,
                name: "".to_string(),
                selector: "/".to_string(),
                host: "fixme".to_string(),
                port: 70,
                url: Url::parse("gopher://fixme:70").unwrap(),
            });
        }
        if l.len() <= 3 {
            // Happens e.g. if a text file is parsed as a gophermap
            return Err("Invalid gophermap entry");
        }
        if l[0].len() == 0 {
            return Err("Invalid gophermap entry, no item type");
        }
        let ch = l[0].chars().next().unwrap();
        let item_type = ItemType::decode(ch);
        let name = l[0][1..].to_string();
        let selector = l[1].to_string();
        let host = l[2].to_string();
        let port = l[3].parse().unwrap();
        let mut path = selector.clone();
        path.insert(0, ch);

        let mut url: Url = Url::parse("gopher://fixme:70").unwrap();
        if item_type == ItemType::Telnet {
            // Telnet URLs have no selector
            url = Url::parse("telnet://fixme:70").unwrap();
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

    pub fn label(self: Self) -> String {
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
    MIME,
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
            'M' => ItemType::MIME,
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
            ItemType::MIME => "[MME]",
            ItemType::Calendar => "[CAL]",
            ItemType::Sound => "[SND]",
            ItemType::Inline => "     ",
            ItemType::Other(_ch) => "[???]",
        }
        .to_string()
    }

    pub fn is_download(item_type: ItemType) -> bool {
        match item_type {
            ItemType::BinHex
            | ItemType::Dos
            | ItemType::Uuencoded
            | ItemType::Binary
            | ItemType::Gif
            | ItemType::Image
            | ItemType::Document
            | ItemType::Video
            | ItemType::MIME
            | ItemType::Calendar
            | ItemType::Sound => true,
            _ => false,
        }
    }

    pub fn is_text(item_type: ItemType) -> bool {
        match item_type {
            ItemType::File => true,
            _ => false,
        }
    }

    pub fn is_dir(item_type: ItemType) -> bool {
        match item_type {
            ItemType::Dir => true,
            _ => false,
        }
    }

    pub fn is_query(item_type: ItemType) -> bool {
        match item_type {
            ItemType::IndexServer => true,
            _ => false,
        }
    }

    pub fn is_inline(item_type: ItemType) -> bool {
        match item_type {
            ItemType::Inline => true,
            _ => false,
        }
    }

    pub fn is_image(item_type: ItemType) -> bool {
        match item_type {
            ItemType::Gif | ItemType::Image => true,
            _ => false,
        }
    }

    pub fn is_telnet(item_type: ItemType) -> bool {
        match item_type {
            ItemType::Telnet => true,
            _ => false,
        }
    }

    pub fn is_html(item_type: ItemType) -> bool {
        match item_type {
            ItemType::Html => true,
            _ => false,
        }
    }

    /// Returns the ItemType of an url. Defaults to gophermap (ItemType::Dir 1)
    pub fn from_url(url: &Url) -> ItemType {
        let path = url.path();
        let mut item_type: ItemType = ItemType::Dir;
        let mut chars = path.chars();
        if path.chars().count() > 2 && chars.next().unwrap() == '/' {
            item_type = ItemType::decode(chars.next().unwrap());
        }
        item_type
    }
}
