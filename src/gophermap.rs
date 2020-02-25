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
    pub fn parse(line: String) -> Self {
        let l: Vec<&str> = line.split_terminator("\t").collect();
        // Sometimes there are empty lines in a gophermap.
        // Return an empty gopher entry so ncgopher does no crash
        // FIXME: Should not have ItemType::File
        if l.len() == 0 {
            return GopherMapEntry {
                item_type: ItemType::File,
                name: "".to_string(),
                selector: "".to_string(),
                host: "error.host".to_string(),
                port: 70,
                url: Url::parse("gopher://no.host:70").unwrap()
            }
        }
        let ch = l[0].chars().next().unwrap();
        let item_type = ItemType::decode(ch);
        let name = l[0][1..].to_string();
        // FIXME Check l.len() in case of malformed gophermap!
        let selector = l[1].to_string();
        let host = l[2].to_string();
        let port = l[3].parse().unwrap();
        let mut url = Url::parse("gopher://fix.me").unwrap();
        if !host.is_empty() {
            url.set_host(Some(host.as_str())).unwrap();
        }
        url.set_port(Some(port)).unwrap();
        url.set_path(selector.as_str());
        GopherMapEntry {
            item_type,
            name,
            selector,
            host,
            port,
            url
        }
    }

    pub fn label(self: Self) -> String {
        self.name.clone()
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
            ch => ItemType::Other(ch),
            // d => Document
            // ; => Video
            // M => MIME
            // c => calendar
            // s => sound
            // i => inline text or info line
        }
    }
}
