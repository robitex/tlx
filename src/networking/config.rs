// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at http://mozilla.org/MPL/2.0/.

// SPDX-License-Identifier: MPL-2.0

/// URL di base e multiplexer CTAN
pub mod ctan {
    // ctan mirror url
    pub const CTAN_MULTIPLEXER: &str = "https://mirror.ctan.org/systems/texlive/tlnet/";
    pub const TLPDB_XZ_RELPATH: &str = "tlpkg/texlive.tlpdb.xz";
    pub const TLPDB_SHA512_RELPATH: &str = "tlpkg/texlive.tlpdb.sha512";
    pub const TLPDB_SHA512_ASC_RELPATH: &str = "tlpkg/texlive.tlpdb.sha512.asc";
}

pub mod keys_pgp {
    // public key ASCII-Armored of TeX Live team
    // downloaded from https://www.tug.org/texlive/files/
    pub const TEXLIVE_PUBKEY: &[u8] = include_bytes!("../../assets/texlive.asc");
}

/// Parametri di rete e HTTP
pub mod network {
    use std::time::Duration;
    //
    /// Timeout di inattività: quanto tempo può passare senza ricevere
    /// un nuovo chunk prima di considerare il download morto.
    pub const CHUNK_INACTIVITY_TIMEOUT: Duration = Duration::from_secs(16);

    pub const USER_AGENT: &str = concat!(
        env!("CARGO_PKG_NAME"),
        "/",
        env!("CARGO_PKG_VERSION"),
        " (+https://github.com/robitex/tlx; giaconet.mailbox@gmail.com)",
    );
}

/// Limiti di memoria e buffer
pub mod memory {
    /// Capacità iniziale consigliata per decompressi tlpdb (~20 MB)
    pub const TLPDB_INITIAL_CAPACITY: usize = 22 * 1024 * 1024;
}
