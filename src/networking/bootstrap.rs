// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at http://mozilla.org/MPL/2.0/.

// SPDX-License-Identifier: MPL-2.0

///! This module has the responsability to download safely the file 'texlive.tlpdb'
///! directly from CTAN mirror system.
///! 'texlive.tlpdb' is a plain text database file with the total of TeX Live tree
use pgp::composed::{Deserializable, DetachedSignature, SignedPublicKey};

use lzma_rs::xz_decompress;
use reqwest::Client;
use sha2::{Digest, Sha512};

use std::io::Cursor;

use crate::networking::config;
use crate::networking::network::download_file;

/// Bootstrap function
/// Download -> check PGP -> check SHA512 -> decompression
pub async fn run_bootstrap(client: &Client, mirror_url: &str) -> std::io::Result<String> {
    use std::io::Error;
    use std::io::ErrorKind::InvalidData;

    println!("🔄 [Phase 0] Downloading 'texlive.tlpdb.xz' from CTAN...");
    println!(" └─ 📥 Download checksum and PGP signature...");

    let sha512_url = format!("{}/{}", mirror_url, config::ctan::TLPDB_SHA512_RELPATH);

    let sha512_bytes = download_file(client, sha512_url).await?;

    let asc_url = format!("{}/{}", mirror_url, config::ctan::TLPDB_SHA512_ASC_RELPATH);

    let asc_bytes = download_file(client, asc_url).await?;

    // Verifica della Firma PGP
    println!(" └─ 🔐 Verifica della firma PGP...");

    verify_pgp_signature(&sha512_bytes, &asc_bytes, config::keys_pgp::TEXLIVE_PUBKEY)?;

    println!("    ✅ Firma PGP autenticata con successo.");

    // Estraiamo la stringa hash attesa dal file .sha512
    let sha512_file_content = String::from_utf8(sha512_bytes.to_vec()).map_err(|e| {
        Error::new(
            InvalidData,
            format!("Il file texlive.tlpdb.sha512 non contiene testo UTF-8 valido: {e}"),
        )
    })?;

    // Download del database compresso texlive.tlpdb.xz
    println!(" └─ 📥 Download del database `texlive.tlpdb.xz`...");
    let tlpdb_xz = config::ctan::TLPDB_XZ_RELPATH;
    let tlpdb_xz_url = format!("{}/{}", mirror_url, tlpdb_xz);

    let tlpdb_xz_bytes = download_file(client, tlpdb_xz_url).await?;

    // Decompressione XZ dello stream direttamente in memoria RAM
    println!(" └─ 📦 Decompressione e verifica XZ di `texlive.tlpdb` in RAM...");
    let ans = decompress_and_verify_tlpdb(&tlpdb_xz_bytes, sha512_file_content.as_bytes())?;
    Ok(ans)
}

/// Helper: Check PGP detouched using signature with `pgp`, a pure Rust library
fn verify_pgp_signature(
    data_bytes: &[u8],
    sig_bytes: &[u8],
    public_keyring_bytes: &[u8],
) -> std::io::Result<()> {
    use std::io::Error;
    use std::io::ErrorKind::InvalidData;

    // Legge il file texlive.asc
    let keyring_str = std::str::from_utf8(public_keyring_bytes).map_err(|e| {
        Error::new(
            InvalidData,
            format!("Il file texlive.asc non è una stringa UTF-8 ASCII-Armored valida: {e}"),
        )
    })?;

    let (public_keys_iter, _) = SignedPublicKey::from_string_many(keyring_str).map_err(|e| {
        Error::new(
            InvalidData,
            format!("Parsing delle chiavi dal file texlive.asc fallito: {e}"),
        )
    })?;

    // Legge la firma staccata (.sha512.asc)
    let sig_str = std::str::from_utf8(sig_bytes).map_err(|e| {
        Error::new(
            InvalidData,
            format!("La firma fornita (.asc) non è UTF-8 ASCII-Armored: {e}"),
        )
    })?;

    let (signature, _headers) =
        DetachedSignature::from_armor_single(sig_str.as_bytes()).map_err(|e| {
            Error::new(
                InvalidData,
                format!("Parsing della firma staccata PGP fallito: {e}"),
            )
        })?;

    let public_keys: Vec<SignedPublicKey> = public_keys_iter.filter_map(|res| res.ok()).collect();

    // Prova a verificare i dati con la chiave primaria o con le sue sottochiavi
    for key in public_keys {
        // Prova diretta con la chiave principale
        if signature.verify(&key, data_bytes).is_ok() {
            return Ok(());
        }

        // Prova con le sottochiavi
        for subkey in &key.public_subkeys {
            if signature.verify(subkey, data_bytes).is_ok() {
                return Ok(());
            }
        }
    }

    Err(Error::new(
        InvalidData,
        "Firma PGP non valida: nessuna chiave nel file texlive.asc corrisponde alla firma.",
    ))
}

pub fn decompress_and_verify_tlpdb(
    compressed_xz_bytes: &[u8],
    sha512_file_bytes: &[u8],
) -> std::io::Result<String> {
    use std::io::Error;
    use std::io::ErrorKind::InvalidData;

    // Decompressione multi-stream in RAM con lzma-rs
    let mut input_cursor = Cursor::new(compressed_xz_bytes);
    let mut decompressed_bytes = Vec::with_capacity(config::memory::TLPDB_INITIAL_CAPACITY);

    // Ciclo per gestire gli stream XZ concatenati che TeX Live usa nei suoi archivi
    while (input_cursor.position() as usize) < compressed_xz_bytes.len() {
        let prev_pos = input_cursor.position();

        // Decomprime il blocco corrente
        if let Err(e) = xz_decompress(&mut input_cursor, &mut decompressed_bytes) {
            // Se non ci sono più dati validi o siamo alla fine del padding, usciamo dal ciclo
            if input_cursor.position() == prev_pos {
                break;
            }
            return Err(Error::new(
                InvalidData,
                format!("Errore durante la trasmissione/decompressione dei blocchi XZ: {e}"),
            ));
        }
    }

    // Estrazione dell'hash atteso dal file .sha512
    let sha512_str = std::str::from_utf8(sha512_file_bytes).map_err(|e| {
        Error::new(
            InvalidData,
            format!("Il file .sha512 non è una stringa UTF-8 valida: {e}"),
        )
    })?;

    let expected_hash = sha512_str
        .split_whitespace()
        .next()
        .unwrap_or("")
        .trim()
        .to_lowercase();

    let calculated_hash = hex::encode(Sha512::digest(&decompressed_bytes));

    // Verifica di corrispondenza dell'hash
    if calculated_hash != expected_hash {
        return Err(Error::new(
            InvalidData,
            format!(
                "Mismatch SHA-512 sul database decompresso!\nDimensione estratta: {} bytes\nAtteso:    {}\nCalcolato: {}",
                decompressed_bytes.len(),
                expected_hash,
                calculated_hash
            ),
        ));
    }

    // Conversione in String
    let tlpdb_string = String::from_utf8(decompressed_bytes).map_err(|e| {
        Error::new(
            InvalidData,
            format!("Il contenuto decompresso non è una stringa UTF-8 valida: {e}"),
        )
    })?;

    Ok(tlpdb_string)
}
