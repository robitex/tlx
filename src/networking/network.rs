// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at http://mozilla.org/MPL/2.0/.

// SPDX-License-Identifier: MPL-2.0

/// utility function
use reqwest::Client;
use reqwest::redirect::Policy;

use bytes::Bytes;

use std::io::{Error, ErrorKind};

/// function to discover the final mirror
/// and build
pub async fn discover_ctan_mirror(ctan_base_url: &str) -> std::io::Result<String> {
    // Creiamo un client temporaneo che NON segue i redirect HTTP
    let no_redirect_client = Client::builder()
        .redirect(Policy::none()) // Blocco automatico sui codice 3xx
        .build()
        .map_err(|err| Error::new(ErrorKind::Other, err.to_string()))?;

    let response = no_redirect_client
        .head(ctan_base_url)
        .send()
        .await
        .map_err(|e| Error::new(ErrorKind::ConnectionRefused, e.to_string()))?;

    // CTAN risponde con 302 Found o 303 See Other
    if response.status().is_redirection() {
        if let Some(location) = response.headers().get("location") {
            let mirror_url = location
                .to_str()
                .map_err(|e| Error::new(ErrorKind::Other, e.to_string()))?;
            return Ok(mirror_url.to_string());
        }
    }

    Err(Error::new(
        ErrorKind::ConnectionAborted,
        format!(
            "errore connessione ctan codice {}",
            response.status().as_u16()
        ),
    ))
}

pub fn create_client() -> std::io::Result<Client> {
    reqwest::Client::builder()
        .user_agent("tlx-texlive-express-installer-in-pure-rust/0.1.0 (https://github.com/robitex/tlx)")
        .timeout(std::time::Duration::from_secs(30))
        .pool_idle_timeout(std::time::Duration::from_secs(90))
        .pool_max_idle_per_host(10)
        .build()
        .map_err(|err| std::io::Error::new(std::io::ErrorKind::Other, err.to_string()))
}

pub async fn download_file(client: &Client, file_url: String) -> std::io::Result<Bytes> {
    use std::io::Error;
    use std::io::ErrorKind::{InvalidData, NotFound, Other};

    let response = client.get(&file_url).send().await.map_err(|e| {
        Error::new(
            Other,
            format!("Errore durante il download del file '{file_url}': {e}"),
        )
    })?;

    // Controllo dello status code prima di scaricare i byte
    if response.status() == reqwest::StatusCode::NOT_FOUND {
        return Err(Error::new(
            NotFound,
            format!("File non trovato presso l'URL: {file_url}"),
        ));
    } else if !response.status().is_success() {
        return Err(Error::new(
            Other,
            format!(
                "Download fallito con status code: {} per il file '{file_url}'",
                response.status()
            ),
        ));
    }

    let bytes = response.bytes().await.map_err(|e| {
        Error::new(
            InvalidData,
            format!("Errore lettura byte: {e} per il file '{file_url}'"),
        )
    })?;

    Ok(bytes)
}
