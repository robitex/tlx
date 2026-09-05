// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at http://mozilla.org/MPL/2.0/.

// SPDX-License-Identifier: MPL-2.0

use ahash::AHashSet;

use std::fs;
use std::io;
use std::path::Path;
use std::process::{Command, Stdio};

use std::fs::File;
use std::io::Write;

use crate::database::database::Database;

const LANGUAGE_DAT: &[u8] = include_bytes!("../../assets/language.dat");
const LANGUAGE_DEF_BEGIN: &[u8] = include_bytes!("../../assets/language_begin.def");
const LANGUAGE_DEF_END: &[u8] = include_bytes!("../../assets/language_end.def");
const LANGUAGE_DAT_LUA: &[u8] = include_bytes!("../../assets/language.dat.lua");

/// enviroment of commands
pub struct InstallContext<'a> {
    install_dir: &'a Path,
    texmf_bin_dir: &'a Path,
    texmf_dist: &'a Path,
    texmf_var: &'a Path,
    texmf_config: &'a Path,
    texmf_home: &'a Path,
}

impl<'a> InstallContext<'a> {
    pub fn setup(
        install_dir: &'a Path,
        texmf_bin_dir: &'a Path,
        texmf_dist: &'a Path,
        texmf_var: &'a Path,
        texmf_config: &'a Path,
        texmf_home: &'a Path,
    ) -> Self {
        InstallContext {
            install_dir,
            texmf_bin_dir,
            texmf_dist,
            texmf_var,
            texmf_config,
            texmf_home,
        }
    }

    // creazione dei file di sillabazione
    pub fn execute_addhyphen(
        &self,
        dataset: &Database,
        pkg_list: &AHashSet<&str>,
    ) -> std::io::Result<()> {
        let dest = self.texmf_var.join("tex").join("generic").join("config");
        fs::create_dir_all(&dest)?;

        // language.dat
        let path_language_dat = dest.join("language.dat");
        let mut language_dat = File::create(&path_language_dat)?;
        language_dat.write_all(LANGUAGE_DAT)?;

        // language.def
        let path_language_def = dest.join("language.def");
        let mut language_def = File::create(&path_language_def)?;
        language_def.write_all(LANGUAGE_DEF_BEGIN)?;

        // language.dat.lua
        let path_language_dat_lua = dest.join("language.dat.lua");
        let mut language_dat_lua = File::create(&path_language_dat_lua)?;
        language_dat_lua.write_all(LANGUAGE_DAT_LUA)?;

        for hyphen in dataset.hyphen_directives_iter(pkg_list) {
            hyphen.write_language_dat(&mut language_dat)?;
            hyphen.write_language_def(&mut language_def)?;
            hyphen.write_language_dat_lua(&mut language_dat_lua)?;
        }

        language_dat.write_all(b"\n")?;
        language_def.write_all(LANGUAGE_DEF_END)?;
        language_dat_lua.write_all(b"}\n")?;

        language_dat.sync_all()?;
        language_def.sync_all()?;
        language_dat_lua.sync_all()?;

        // build ls-R index file for texmf-var
        // this file will be re-written later because generation of maps font
        // and formats write in this system directory but at the same time
        // they needs ls-R for searching during their execution
        let mut tree = crate::execute::tree::Node::default();
        tree.insert_file_path(&path_language_dat);
        tree.insert_file_path(&path_language_def);
        tree.insert_file_path(&path_language_dat_lua);

        let lsr_path = self.texmf_var.join("ls-R");
        let file = std::fs::File::create(&lsr_path)?;

        let mut writer = std::io::BufWriter::with_capacity(64 * 1024, file);
        tree.generate_texmf_dist_ls_r(&mut writer)?;

        Ok(())
    }

    /// Configura ed esegue un comando TeX Live garantendo l'ambiente corretto.
    pub fn run_command(&self, program: &str, args: &[&str]) -> io::Result<()> {
        let program_path = self.texmf_bin_dir.join(program);

        // prepariamo la variabile PATH inserendo la cartella dei binari TeX Live in testa
        // let current_path = std::env::var_os("PATH").unwrap_or_default();
        // let new_path = std::env::join_paths(
        //    std::iter::once(self.bin_dir.to_path_buf()).chain(std::env::split_paths(&current_path)),
        // )
        // .map_err(|e| io::Error::new(io::ErrorKind::Other, e))?;

        println!("PATH child: {:?}", &self.texmf_bin_dir);

        let output = Command::new(&program_path)
            .env_clear()
            .args(args)
            .env("PATH", &self.texmf_bin_dir)
            .env("TEXMFROOT", &self.install_dir)
            .env("TEXMFDIST", &self.texmf_dist)
            .env("TEXMFSYSVAR", &self.texmf_var)
            .env("TEXMFSYSCONFIG", &self.texmf_config)
            .env("TEXMFHOME", &self.texmf_home)
            // variabili OS-level necessarie non legate a nessuna installazione TeX Live esistente
            .env(
                "WINDIR",
                std::env::var("WINDIR").unwrap_or_else(|_| r"C:\Windows".into()),
            )
            .env(
                "SystemRoot",
                std::env::var("SystemRoot").unwrap_or_else(|_| r"C:\Windows".into()),
            )
            .env("TEMP", std::env::var("TEMP").unwrap_or_default())
            .env("TMP", std::env::var("TMP").unwrap_or_default())
            .env(
                "USERPROFILE",
                std::env::var("USERPROFILE").unwrap_or_default(),
            )
            .env(
                "PATHEXT",
                std::env::var("PATHEXT").unwrap_or_else(|_| ".COM;.EXE;.BAT;.CMD".to_string()),
            )
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .output()?;

        if !output.status.success() {
            return Err(io::Error::new(
                io::ErrorKind::Other,
                format!(
                    "Il comando 'updmap-sys' è fallito con codice: {:?}\nstderr: {}\nstdout: {}",
                    output.status.code(),
                    String::from_utf8_lossy(&output.stderr),
                    String::from_utf8_lossy(&output.stdout),
                ),
            ));
        } else {
            println!(
                "stdout fmtutil-sys: {}",
                String::from_utf8_lossy(&output.stdout)
            );
            println!(
                "stderr fmtutil-sys: {}",
                String::from_utf8_lossy(&output.stderr)
            );
        }
        Ok(())
    }
}
