use std::path::Path;
use url::Url;

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

/// Transforms a URL back into its human readable Unicode representation.
pub fn human_readable_url(url: &Url) -> String {
    match url.scheme() {
        // these schemes are considered "special" by the WHATWG spec
        // cf. https://url.spec.whatwg.org/#special-scheme
        "ftp" | "http" | "https" | "ws" | "wss" => {
            // first unescape the domain name from IDNA encoding
            let url_str = if let Some(domain) = url.domain() {
                let (domain, result) = idna::domain_to_unicode(domain);
                result.expect("could not decode idna domain");
                let url_str = url.to_string();
                // replace the IDNA encoded domain with the unescaped version
                // since the domain cannot contain percent signs we do not have
                // to worry about double unescaping later
                url_str.replace(url.host_str().unwrap(), &domain)
            } else {
                // must be using IP address
                url.to_string()
            };
            // now unescape the rest of the URL
            percent_encoding::percent_decode_str(&url_str)
                .decode_utf8()
                .unwrap()
                .to_string()
        }
        _ => {
            // the domain and the path will be percent encoded
            // it is easiest to do it all at once
            percent_encoding::percent_decode_str(url.as_str())
                .decode_utf8_lossy()
                .into_owned()
        }
    }
}

/// Returns a path into the configured download directory with either
/// the file name in the Url
pub fn download_filename_from_url(url: &Url) -> String {
    let download_path = crate::SETTINGS.read().unwrap().config.download_path.clone();

    let filename = match url.path_segments() {
        Some(path_segments) => path_segments.last().unwrap_or_default(),
        None => "download",
    };
    let filename = if filename.is_empty() {
        // FIXME: file extension based on mime type
        "download"
    } else {
        filename
    };

    let path = Path::new(&download_path).join(filename);
    path.display().to_string()
}
