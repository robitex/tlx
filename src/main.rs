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
    pub mod pipeline;
    pub mod network;
}

mod execute {
    pub mod addformat;
    pub mod addhyphen;
    pub mod command;
    pub mod tree;
}

mod location;

use crate::{
    database::database::Database,
    execute::command::{FormatOutcome::IoError, FormatOutcome::NonZeroExit},
    networking::{bootstrap, config, network},
};

use tracing::info;
use tracing_subscriber::{EnvFilter, fmt, prelude::*};

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

    // directory di installazione locale utente
    // oppure la stessa dove si lancia tlx
    let home_dir = location::get_default_home().unwrap_or(std::env::current_dir()?);
    std::fs::create_dir_all(&home_dir)?;

    let home_dir = dunce::canonicalize(&home_dir)?;

    // creation of the installation context
    let year = db.config_options.release;
    let install_context = location::InstallContext::setup(&home_dir, year, target_arch);
    let install_dir = &install_context.install_dir;

    println!("Install directory '{:?}'", install_dir);

    // phase 3: log
    // configura l'appender per il file di log.
    // `never` significa un singolo file di log per l'installazione
    let file_appender = tracing_appender::rolling::never(install_dir, "tlx-install.log");

    // rendi la scrittura del log non bloccante
    // IMPORTANTE: Mantieni `_guard` nello scope principale fino al termine dell'applicazione
    let (non_blocking, _guard) = tracing_appender::non_blocking(file_appender);

    // inizializza il subscriber (invia su file)
    tracing_subscriber::registry()
        // Livello di log predefinito (INFO), impostabile con variabile d'ambiente
        // RUST_LOG senza ricompilare
        .with(EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("info")))
        // Layer di formattazione per il file
        .with(
            fmt::layer().with_writer(non_blocking).with_ansi(false), // Disattiva i colori ANSI nei file di testo
        )
        // Opzionale: stampa contemporaneamente su terminale (stdout)
        // .with(fmt::layer().with_writer(std::io::stdout))
        .init();

    info!("=== Avvio processo di installazione TLX ===");

    // dipendenze
    // phase 4: dependencies
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

    // phase 5: launch the installer
    let start_time = std::time::Instant::now();
    println!("Avvio della Pipeline di Download ed Estrazione...");

    // eseguiamo la pipeline
    networking::pipeline::run_pipeline(&client, &mirror_url, &db, &pkg_list, &install_dir, 12)
        .await
        .map_err(|e| std::io::Error::new(std::io::ErrorKind::Other, e.to_string()))?;

    let elapsed = start_time.elapsed();
    println!("Installazione completata in {:?}!", elapsed);

    // phase 6: creazione del file locale texlive.tlpdb
    println!("Avvio creazione del file locale texlive.tlpdb...");

    let tlpdb_path = install_context.local_tlpdb_dir.join("texlive.tlpdb");
    let tlpdb_file = std::fs::File::create(&tlpdb_path)?;
    let mut writer_tlpdb = std::io::BufWriter::with_capacity(64 * 1024, tlpdb_file);
    db.write_tlpdb(&mut writer_tlpdb, &pkg_list)?;

    println!("Fine");

    // phase 7: execute configuration program
    // compiti di fine installazione

    // creazione dei file di sillabazione
    // create language.dat, language.def, language.dat.lua files
    // and save ls-R index file for texmf-var directory
    println!("Avvio creazione del file di sillabazione...");
    execute::command::write_addhyphen(&install_context, &db, &pkg_list)?;
    println!("Fine");

    // if context is installed run mtxrun
    if pkg_list.contains("context") {
        println!("Avvio esecuzione di mtxrun...");
        execute::command::run_command(&install_context, "mtxrun", &["--generate"])?;
        execute::command::run_command(&install_context, "mtxrun", &["--luatex", "--generate"])?;
        println!("Fine");
    }

    // build map of fonts
    println!("Avvio creazione delle mappe dei font...");
    execute::command::run_command(&install_context, "updmap-sys", &["--nohash"])?;
    println!("Fine");

    // generazione dei formati
    let t_fmt_start = std::time::Instant::now();
    println!("Avvio creazione dei formati...");

    // scrittura del file fmtutil.cnf, return the format specifications
    let add_formats = execute::command::write_addformat(&install_context, &db, &pkg_list)?;
    let fmt_results = execute::command::build_all_formats(&install_context, &add_formats).await;
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
    let texmf_var_tree = crate::execute::tree::Node::from_dir(&install_context.texmf_var)?;

    let lsr_path = install_context.texmf_var.join("ls-R");
    let file = std::fs::File::create(&lsr_path)?;

    let mut writer = std::io::BufWriter::with_capacity(64 * 1024, file);
    texmf_var_tree.generate_texmf_dist_ls_r(&mut writer)?;

    // directory necessaria per il funzionamento di tlmgr
    let tlpkg_bak_dir = install_dir.join("tlpkg").join("backups");
    std::fs::create_dir_all(&tlpkg_bak_dir)?;

    // final message
    let year = db.config_options.release;
    println!("Welcome in TeX Live {year}!");
    println!(
        "See {}/index.html for links to documentation.",
        install_dir.display()
    );
    Ok(())
}
