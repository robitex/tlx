// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at http://mozilla.org/MPL/2.0/.

// SPDX-License-Identifier: MPL-2.0

use std::path::{Path, PathBuf};

pub struct InstallContext {
    pub install_dir: PathBuf,
    pub texmf_bin: PathBuf,
    pub texmf_dist: PathBuf,
    pub texmf_var: PathBuf,
    pub texmf_config: PathBuf,
    pub texmf_home: PathBuf,
    //
    pub local_tlpdb_dir: PathBuf,
    pub language_file_dir: PathBuf,
    pub fmtutil_cnf_dir: PathBuf,
}

pub fn get_default_home() -> Option<PathBuf> {
    // su Windows restituisce C:\Users\<Utente>\AppData\Local
    // su Linux ~/.local/share
    // su macOS ~/Library/Application Support
    let local_dir = dirs::data_local_dir()?;

    // sub-directory of installation: the home of TeX Live
    let ans = local_dir.join("texlive");
    Some(ans)
}

impl InstallContext {
    pub fn setup(home: &Path, year: u16, target_arch: &str) -> Self {
        // main install directories
        let install_dir = home.join(year.to_string());
        let texmf_bin = install_dir.join("bin").join(target_arch);
        let texmf_dist = install_dir.join("texmf-dist");
        let texmf_var = install_dir.join("texmf-var");
        let texmf_config = install_dir.join("texmf-config");
        let texmf_home = home.join("texmf-local");
        // texlive.tlpdb local position
        let local_tlpdb_dir = install_dir.join("tlpkg");
        // language.dat, ..., location
        let language_file_dir = texmf_var.join("tex").join("generic").join("config");
        // fmtutil_cnf location
        let fmtutil_cnf_dir = texmf_dist.join("web2c");

        InstallContext {
            install_dir,
            texmf_bin,
            texmf_dist,
            texmf_var,
            texmf_config,
            texmf_home,
            // other specific locations
            local_tlpdb_dir,
            language_file_dir,
            fmtutil_cnf_dir,
        }
    }
}
