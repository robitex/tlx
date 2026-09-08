// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at http://mozilla.org/MPL/2.0/.

// SPDX-License-Identifier: MPL-2.0

mod database {
    pub mod arch;
    pub mod database;
    pub mod package;
}

mod networking {
    pub mod bootstrap;
    pub mod config;
    pub mod installer;
    pub mod network;
}

mod execute {
    pub mod addformat;
    pub mod addhyphen;
    pub mod command;
    pub mod tree;
}

use crate::{
    database::database::Database,
    execute::command::{self, FormatOutcome::IoError, FormatOutcome::NonZeroExit},
    networking::{bootstrap, config, network},
};

// main function
#[tokio::main]
async fn main() -> std::io::Result<()> {
    // phase 1: getting texlive.tlpdb file content
    let ctan_base_url = config::ctan::CTAN_MULTIPLEXER;
    let mirror_url = network::discover_ctan_mirror(ctan_base_url).await?;
    let client = network::create_client()?;
    let texlive_tlpdb_content = bootstrap::run_bootstrap(&client, &mirror_url).await?;

    // phase 2: parsing
    let target_arch = "windows";
    let include_doc = true;
    let include_src = true;
    let db = Database::from_tlpdb(
        &texlive_tlpdb_content,
        target_arch,
        include_doc,
        include_src,
        &mirror_url,
        false,
    );
    // debug!
    println!("Pacchetti letti: {}", db.len());
    println!("");

    // dipendenze
    // phase 3: dependencies
    println!("Dipendenze:");

    let scheme = if target_arch == "windows" {
        vec!["scheme-full", "collection-wintools"]
    } else {
        vec!["scheme-full"]
    };

    let mut pkg_list = db.resolve_dep(&scheme);

    // aggiunta manuale di pacchetti extra a Windows
    if target_arch == "windows" {
        let windows_extra = ["tlperl.windows", "tlgs.windows"];
        pkg_list.extend(windows_extra);
    }

    println!("Numero pacchetti dello scheme-full: {}", pkg_list.len());

    // phase 4: launch the installer
    let start_time = std::time::Instant::now();

    let home_dir = std::path::PathBuf::from("smoke-test").join("texlive");
    std::fs::create_dir_all(&home_dir)?;

    // make home_dir an absolute path
    let home_dir = dunce::canonicalize(&home_dir)?;

    let install_dir = home_dir.join("2026");

    let texmf_bin_dir = install_dir.join("bin").join("windows");
    let texmf_dist_dir = install_dir.join("texmf-dist");
    let texmf_var_dir = install_dir.join("texmf-var");
    let texmf_config_dir = install_dir.join("texmf-config");
    let texmf_home = home_dir.join("texmf-local");

    println!("Install directory '{:?}'", install_dir);
    println!("Avvio della Pipeline di Download ed Estrazione...");

    // eseguiamo la pipeline
    networking::installer::run_pipeline(&client, &mirror_url, &db, &pkg_list, &install_dir, 8)
        .await
        .map_err(|e| std::io::Error::new(std::io::ErrorKind::Other, e.to_string()))?;

    let elapsed = start_time.elapsed();
    println!("Installazione completata in {:?}!", elapsed);

    // phase 5: creazione del file locale texlive.tlpdb
    println!("Avvio creazione del file locale texlive.tlpdb...");
    let tlpdb_path = install_dir.join("tlpkg").join("texlive.tlpdb");
    let tlpdb_file = std::fs::File::create(&tlpdb_path)?;
    let mut writer_tlpdb = std::io::BufWriter::with_capacity(64 * 1024, tlpdb_file);
    db.write_tlpdb(&mut writer_tlpdb, &pkg_list)?;
    println!("Fine");

    // phase 6: execute configuration program
    // compiti di fine installazione
    // creazione dei file di sillabazione
    let cmd = command::InstallContext::setup(
        &install_dir,
        &texmf_bin_dir,
        &texmf_dist_dir,
        &texmf_var_dir,
        &texmf_config_dir,
        &texmf_home,
    );

    // create language.dat, language.def, language.dat.lua files
    // and save ls-R index file for texmf-var directory
    println!("Avvio creazione del file di sillabazione...");
    cmd.write_addhyphen(&db, &pkg_list)?;
    println!("Fine");

    // if context is installed run mtxrun
    if pkg_list.contains("context") {
        println!("Avvio esecuzione di mtxrun...");
        cmd.run_command("mtxrun", &["--generate"])?;
        cmd.run_command("mtxrun", &["--luatex", "--generate"])?;
        println!("Fine");
    }

    // build map of fonts
    println!("Avvio creazione delle mappe dei font...");
    cmd.run_command("updmap-sys", &["--nohash"])?;
    println!("Fine");

    // generazione dei formati
    let t_fmt_start = std::time::Instant::now();
    println!("Avvio creazione dei formati...");

    // scrittura del file fmtutil.cnf, return the format specifications
    let add_formats = cmd.write_addformat(&db, &pkg_list)?;
    let fmt_results = cmd.build_all_formats(&add_formats).await;
    for result in &fmt_results {
        let fmt_name = result.name.as_str();
        let outcome = &result.outcome;
        match outcome {
            NonZeroExit {
                code,
                stdout,
                stderr,
            } => println!(
                "Errore generazione formato {fmt_name}: code={:?} stdout={stdout} stderr={stderr}",
                code
            ),
            IoError(err) => println!("Errore generazione formato su I/O: {err}"),
            _ => {}
        }
    }
    println!(
        "End build of {} formats in {:?}",
        add_formats.len(),
        t_fmt_start.elapsed()
    );

    // creazione del file ls-R per texmf-var in cui i comandi precedenti hanno scritto
    // build ls-R index file for texmf-var
    let texmf_var_tree = crate::execute::tree::Node::from_dir(&texmf_var_dir)?;

    let lsr_path = texmf_var_dir.join("ls-R");
    let file = std::fs::File::create(&lsr_path)?;

    let mut writer = std::io::BufWriter::with_capacity(64 * 1024, file);
    texmf_var_tree.generate_texmf_dist_ls_r(&mut writer)?;

    // directory necessaria per il funzionamento di tlmgr
    let tlpkg_bak_dir = install_dir.join("tlpkg").join("backups");
    std::fs::create_dir_all(&tlpkg_bak_dir)?;

    // final message
    let year = db.options.release;
    println!("Welcome in TeX Live {year}!");
    println!(
        "See {}/index.html for links to documentation.",
        install_dir.display()
    );
    Ok(())
}

// Numero pacchetti dello scheme-full: 5111
// ---- stampa i pacchetti avanzati: non sono nelle dipendenze, da controllare
//
// pacchetto fuori lista 00texlive.installation
// pacchetto fuori lista 00texlive.config
//
// pacchetto fuori lista scheme-context
// pacchetto fuori lista scheme-bookpub
// pacchetto fuori lista scheme-tetex
// pacchetto fuori lista scheme-minimal
// pacchetto fuori lista scheme-basic
// pacchetto fuori lista scheme-medium
// pacchetto fuori lista scheme-gust
// pacchetto fuori lista scheme-small
// pacchetto fuori lista scheme-infraonly
//
// pacchetto fuori lista tlperl.windows
// pacchetto fuori lista tlgs.windows
//
// pacchetto fuori lista collection-wintools
// pacchetto fuori lista wintools.windows
// pacchetto fuori lista dviout.windows
