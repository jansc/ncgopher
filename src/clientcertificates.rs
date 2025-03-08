use ::time::Date;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::io::Write;
use std::path::Path;
use url::Url;

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ClientCertificate {
    pub fingerprint: String,
    pub cert: String,
    pub private_key: String,
    pub common_name: String,
    pub expiration_date: Date,
    pub note: String,
}

#[derive(Clone, Debug, Serialize, Deserialize, Default)]
pub struct ClientCertificates {
    /// Maps URLs to certificate fingerprints
    // Each certificate can be valid for several URLs
    #[serde(rename = "urls", default = "default_urls")]
    pub urls: HashMap<String, String>,
    /// All known server certificates. Hash where the key is the
    /// Certificate fingerprint.
    #[serde(rename = "certificates", default = "default_certificates")]
    pub certificates: HashMap<String, ClientCertificate>,
}

fn default_certificates() -> HashMap<String, ClientCertificate> {
    HashMap::<String, ClientCertificate>::new()
}

fn default_urls() -> HashMap<String, String> {
    HashMap::<String, String>::new()
}

impl ClientCertificates {
    pub fn new() -> ClientCertificates {
        let confdir = ClientCertificates::get_client_certificates_filename();
        let mut config_string = String::new();
        if Path::new(confdir.as_str()).exists() {
            config_string = std::fs::read_to_string(&confdir).unwrap_or_default();
        }
        toml::from_str(&config_string).unwrap_or_default()
    }

    fn get_client_certificates_filename() -> String {
        let confdir: String = match dirs::config_dir() {
            Some(mut dir) => {
                dir.push(env!("CARGO_PKG_NAME"));
                dir.push("client_certificates");
                dir.into_os_string().into_string().unwrap()
            }
            None => String::new(),
        };
        info!("Looking for client_certificates file {}", confdir);
        confdir
    }

    /// Add or replace the fingerprint that would be used for the given
    /// normalized URL.
    pub fn insert(&mut self, client_certificate: ClientCertificate, specified_url: &Option<Url>) {
        let fingerprint = client_certificate.fingerprint.to_string();
        self.certificates.insert(
            client_certificate.fingerprint.to_string(),
            client_certificate,
        );
        if let Some(url) = specified_url {
            self.urls.insert(url.to_string(), fingerprint);
        }
        if let Err(why) = self.write_to_file() {
            warn!("Could not write client_certificates to file: {}", why)
        }
    }

    pub fn update(&mut self, cc: &ClientCertificate, urls: Vec<Url>) {
        let fingerprint = &cc.fingerprint;
        self.urls.retain(|_k, v| !&v.eq(&fingerprint));
        for url in urls.iter() {
            self.urls.insert(url.to_string(), fingerprint.to_string());
        }
        self.certificates
            .insert(fingerprint.to_string(), cc.clone());
        if let Err(why) = self.write_to_file() {
            warn!("Could not write client_certificates to file: {}", why)
        }
    }

    pub fn get_client_certificate_fingerprint(&mut self, url: &Url) -> Option<String> {
        if let Some(fingerprint) = self.urls.get(url.as_str()) {
            if self.certificates.contains_key(fingerprint) {
                return Some(fingerprint.to_string());
            }
        }
        None
    }

    pub fn get_cert_by_fingerprint(&mut self, fingerprint: &String) -> Option<String> {
        if let Some(cc) = self.certificates.get(fingerprint) {
            return Some(cc.cert.to_string());
        }
        None
    }

    pub fn get_private_key_by_fingerprint(&mut self, fingerprint: &String) -> Option<String> {
        if let Some(cc) = self.certificates.get(fingerprint) {
            return Some(cc.private_key.to_string());
        }
        None
    }

    /// Returns a vector with all client certficiates
    pub fn get_client_certificates(&self) -> Vec<ClientCertificate> {
        self.certificates.clone().into_values().collect()
    }

    /// Returns a vector with all client certficiates
    pub fn get_client_certificate(&self, fingerprint: &String) -> Option<ClientCertificate> {
        if let Some(cc) = self.certificates.get(fingerprint) {
            return Some(cc.clone());
        }
        None
    }

    /// Returns a list of URLs that are assigned to a given client certificate
    /// identified by a fingerprint
    pub fn get_urls_for_certificate(&self, fingerprint: &String) -> Vec<String> {
        let mut map: HashMap<String, String> = self.urls.clone();
        map.retain(|_k, v| v.eq(&fingerprint));
        map.into_keys().collect()
    }

    /// Removes a certificate and its associated URLs
    pub fn remove(&mut self, fingerprint: &String) {
        info!("Removing entry to client certificate: {:?}", fingerprint);
        self.urls.retain(|_k, v| !&v.eq(&fingerprint));
        self.certificates
            .retain(|_k, v| &v.fingerprint != fingerprint);
        if let Err(why) = self.write_to_file() {
            warn!("Could not write client certificate file: {}", why)
        }
    }

    pub fn use_current_site(&mut self, url: &Url, fingerprint: &String) {
        info!("Adding {:?} to {}", url, fingerprint);
        self.urls.insert(url.to_string(), fingerprint.to_string());
        if let Err(why) = self.write_to_file() {
            warn!("Could not write client certificate file: {}", why)
        }
    }

    /// Writes all client certificates held by this instance to a toml-file.
    pub fn write_to_file(&mut self) -> std::io::Result<()> {
        let filename = ClientCertificates::get_client_certificates_filename();
        info!("Saving client_certificates to file: {}", filename);
        // Create a path to the desired file
        let path = Path::new(&filename);

        let mut file = std::fs::File::create(path)?;

        file.write_all(b"# Automatically generated by ncgopher.\n")?;
        file.write_all(
            toml::to_string(&self)
                .expect("known hosts could not be stored as TOML")
                .as_bytes(),
        )?;
        Ok(())
    }
}
