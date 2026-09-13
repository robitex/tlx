// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at http://mozilla.org/MPL/2.0/.

// SPDX-License-Identifier: MPL-2.0

use std::fmt;
use std::fs;
use std::fs::File;
use std::io::{Read, Write};
use std::path::Path;
use std::path::PathBuf;
use std::sync::Arc;

use bytes::{Bytes, BytesMut};
use futures_util::StreamExt;
use reqwest::Client;
use sha2::{Digest, Sha512};
use tar::Archive;
use tar::EntryType;
use tokio::sync::mpsc;
use tokio::task::JoinHandle;
use tokio::time::timeout;
use tracing::{error, info};

use crate::database::database::Database;
use crate::execute::tree;
use crate::networking::config;

// file da scaricare per lo stadio 1
pub struct RemoteFile {
    name: String,
    expected_sha512: String,
    relocated: bool,
}

/// Messaggio dallo Stadio 1 -> Stadio 2
pub struct CompressedPackage {
    name: String,
    bytes: Bytes,
    relocated: bool,
}

/// dal stadio 2 -> per lo stadio 3
enum WritePayload {
    File {
        name: String,
        routed_path: PathBuf,
        data: Vec<u8>,
    },
    Directory {
        name: String,
        routed_path: PathBuf,
    },
}

// eventi della pipeline per logging e tracciamento
#[derive(Debug)]
#[allow(dead_code)]
pub enum PipelineEvent {
    ProducerNote {
        counter: usize,
    },
    DownloadStarted {
        id_worker: usize,
        pkg_name: String,
    },
    DownloadFinished {
        id_worker: usize,
        pkg_name: String,
        bytes_len: usize,
    },
    DownloadFailed {
        id_worker: usize,
        pkg_name: String,
        error: String,
    },

    ExtractionStarted {
        id_worker: usize,
        pkg_name: String,
    },
    ExtractionFinished {
        id_worker: usize,
        pkg_name: String,
    },
    ExtractionFailed {
        id_worker: usize,
        pkg_name: String,
        error: String,
    },
}

impl RemoteFile {
    pub fn new(name: String, expected_sha512: String, relocated: bool) -> Self {
        RemoteFile {
            name,
            expected_sha512,
            relocated,
        }
    }
}

/// funzione principale per la costruzione della pipeline asincrona
pub async fn run_pipeline<'a>(
    client: &Client,
    mirror_url: &str,
    database: &Database<'a>,
    pkg_to_download: &ahash::AHashSet<&str>,
    install_dir: &PathBuf,
    max_concurrent_downloads: usize,
) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    let mirror_url = Arc::new(mirror_url.to_string());

    // Canale Eventi / Log
    let (tx_event, mut event_rx) = mpsc::channel::<PipelineEvent>(2048);

    let logger_handle: JoinHandle<()> = tokio::spawn(async move {
        while let Some(event) = event_rx.recv().await {
            match event {
                PipelineEvent::ProducerNote { counter } => {
                    println!("[Producer] sent job number {counter}")
                }
                PipelineEvent::DownloadStarted {
                    id_worker,
                    pkg_name,
                } => println!("[task {id_worker}] starting download of '{pkg_name}'"),
                PipelineEvent::DownloadFinished {
                    id_worker,
                    pkg_name,
                    bytes_len,
                } => {
                    println!("[task {id_worker}] Download OK for '{pkg_name}' ({bytes_len} bytes)")
                }
                PipelineEvent::DownloadFailed {
                    id_worker,
                    pkg_name,
                    error,
                } => println!("[task {id_worker}] Download Failed for '{pkg_name}': {error}"),
                PipelineEvent::ExtractionStarted {
                    id_worker,
                    pkg_name,
                } => println!("[extractor {id_worker}] unpack '{pkg_name}'"),
                PipelineEvent::ExtractionFinished {
                    id_worker,
                    pkg_name,
                } => println!("[extractor {id_worker}] ESTRAZIONE OK for '{pkg_name}'"),
                PipelineEvent::ExtractionFailed {
                    id_worker,
                    pkg_name,
                    error,
                } => println!("[extractor {id_worker}] Errorre su '{pkg_name}': {error}"),
            };
        }
    });

    // MPMC nativo per i job di download ed estrazione tramite async_channel (o flussi di task spawner)
    let (job_tx, job_rx) = async_channel::bounded::<RemoteFile>(128);

    let (extract_tx_mpmc, rx_extract_mpmc) =
        async_channel::bounded::<CompressedPackage>(max_concurrent_downloads);

    let remote_files: Vec<_> = database
        .remote_files_iter(pkg_to_download)
        // .take(256) // limited for test purposes
        .collect();
    println!(
        "[pipeline] Downloading {} compressed files.",
        remote_files.len()
    );

    // STADIO 0: Producer
    // let event_tx_for_producer = tx_event.clone();
    let handle_sender = tokio::spawn(async move {
        let mut counter = 0;
        for remote_file in remote_files {
            let file = remote_file.name.clone();
            match job_tx.send(remote_file).await {
                Ok(()) => {
                    counter += 1;

                    // if event_tx_for_producer
                    //     .send(PipelineEvent::ProducerNote { counter })
                    //     .await
                    //     .is_err()
                    // {
                    //     break;
                    // };

                    info!("[PR] starting run #{counter} for '{file}'");
                }
                Err(send_err) => {
                    println!(
                        "[producer] Impossibile inviare: il ricevitore è stato chiuso. {send_err}"
                    );
                }
            };
        }
        // event_tx_for_producer viene droppato automaticamente qui alla fine dello scope del task
    });

    // STADIO 1: Worker di Download
    let mut download_handles = Vec::with_capacity(max_concurrent_downloads);

    for id_worker in 0..max_concurrent_downloads {
        // let event_tx_clone = tx_event.clone();
        let job_rx_clone = job_rx.clone();
        let extract_tx_clone = extract_tx_mpmc.clone();
        let client_clone = client.clone();
        let mirror_url = Arc::clone(&mirror_url);

        let handle = tokio::spawn(async move {
            // clousure
            while let Ok(pkg) = job_rx_clone.recv().await {
                let url = format!("{}archive/{}", mirror_url, pkg.name);
                let sha512 = pkg.expected_sha512;

                // no more messages on event channel
                // nlet _result = event_tx_clone
                //    .send(PipelineEvent::DownloadStarted {
                //        id_worker,
                //        pkg_name: pkg.name.clone(),
                //    })
                //    .await;

                info!("[DL {id_worker}] downloading file '{}'", pkg.name);

                match download_file(&client_clone, &url, &sha512).await {
                    Ok(bytes) => {
                        let bytes_len = bytes.len();

                        // let _ = event_tx_clone
                        //    .send(PipelineEvent::DownloadFinished {
                        //        id_worker,
                        //        pkg_name: pkg.name.clone(),
                        //        bytes_len,
                        //    })
                        //    .await;

                        info!(
                            "[DL {id_worker}] download complete: '{}' ({bytes_len} bytes)",
                            pkg.name
                        );

                        // invio del file alla fase di decompressione
                        let extract_job = CompressedPackage {
                            name: pkg.name,
                            bytes,
                            relocated: pkg.relocated,
                        };
                        if extract_tx_clone.send(extract_job).await.is_err() {
                            break;
                        }
                    }
                    Err(err) => {
                        error!(
                            "[DL {id_worker}] fail to download '{}': {}",
                            pkg.name,
                            err.to_string()
                        );

                        // let _ = event_tx_clone
                        //     .send(PipelineEvent::DownloadFailed {
                        //         id_worker,
                        //         pkg_name: pkg.name,
                        //         error: err.to_string(),
                        //     })
                        //     .await;
                    }
                }
            }
            drop(extract_tx_clone);
        });
        download_handles.push(handle);
    }

    // STADIO 2: Worker di Decompressione
    let num_extract_workers = std::thread::available_parallelism()
        .map(|n| n.get())
        .unwrap_or(4); // Fallback ragionevole in caso raro di errore I/O

    let mut extract_handles = Vec::with_capacity(num_extract_workers);

    // Canale:
    // Stadio 2 (Unpacking) -> Stadio 3 (Disk Writer)
    let (tx_write_payload, rx_write_payload) = mpsc::channel::<WritePayload>(4096);

    for id_worker in 0..num_extract_workers {
        let rx_extract_clone = rx_extract_mpmc.clone();
        // let tx_event_clone = tx_event.clone();
        let tx_archives_clone = tx_write_payload.clone();

        // worker dello stadio 2: unpacking dei file
        let handle = tokio::task::spawn_blocking(move || {
            while let Ok(compressed_pkg) = rx_extract_clone.recv_blocking() {
                let pkg_name = compressed_pkg.name;

                // let _ = tx_event_clone.blocking_send(PipelineEvent::ExtractionStarted {
                //    id_worker,
                //    pkg_name: pkg_name.clone(),
                // });

                info!("[XZ {id_worker}] extracting '{pkg_name}'");

                let bytes = compressed_pkg.bytes;
                let relocated = compressed_pkg.relocated;

                // chiamata della funzione principale del secondo stadio della pipeline
                match unpack_and_route(
                    id_worker,
                    pkg_name.clone(),
                    bytes,
                    relocated,
                    &tx_archives_clone,
                ) {
                    Ok(()) => {
                        // let _ = tx_event_clone.send(PipelineEvent::ExtractionFinished {
                        //     id_worker,
                        //     pkg_name: pkg_name,
                        // });

                        // info!("[XZ {id_worker}] '{pkg_name}' extracted");
                    }
                    Err(err_msg) => {
                        // Log dell'errore sul pacchetto senza interrompere l'intera pipeline
                        // let _ = tx_event_clone.send(PipelineEvent::ExtractionFailed {
                        //     id_worker,
                        //     pkg_name,
                        //     error: err_msg,
                        // });

                        error!("[XZ {id_worker}] failed extraction for '{pkg_name}': {err_msg}");
                    }
                };
            }
        });

        extract_handles.push(handle);
    }

    // stadio 3: salvataggio archivio tar su disco
    // worker singolo
    let handle_disk_writer = spawn_disk_writer(install_dir.clone(), rx_write_payload);

    // Attendi il produttore iniziale e i download worker
    // attesa chiusura del sender
    handle_sender
        .await
        .expect("Il task del producer è andato in panic");

    for h in download_handles {
        match h.await {
            Ok(_) => {}
            Err(e) if e.is_panic() => {
                eprintln!("Un worker di download è andato in panic!");
            }
            Err(e) => eprintln!("Worker di download terminato con errore: {}", e),
        };
    }

    // Nessun nuovo pacchetto verrà più inviato a `extract_rx_mpmc`.
    // Droppiamo sia il receiver principale sia l'eventuale sender rimasto nel main scope!
    rx_extract_mpmc.close();
    drop(extract_tx_mpmc);

    // 2. I download sono finiti. 'extract_tx_mpmc' è stato già droppato nel main.
    // Attendiamo che TUTTI i worker di estrazione svuotino la coda e terminino.
    for h in extract_handles {
        match h.await {
            Ok(_) => {}
            Err(e) if e.is_panic() => {
                eprintln!("Un worker di estrazione è andato in panic!");
            }
            Err(e) => eprintln!("Worker di setrazione terminato con errore: {}", e),
        };
    }

    // Droppiamo la copia di 'tx_files' che risiedeva nello scope di run_pipeline
    drop(tx_write_payload);

    // ORA 'rx_files' nel Disk Writer vede 0 trasmettitori attivi, riceve None e termina!
    handle_disk_writer
        .await
        .map_err(|e| format!("Errore Join Disk Writer: {e}"))??;

    // Chiudiamo il trasmettitore del logger per fare uscire anche il logger
    drop(tx_event);

    logger_handle
        .await
        .map_err(|e| format!("Errore di Join del Logger: {e}"))?;

    Ok(())
}

// Tipo di errore dedicato per la fase di download e checksum
#[derive(Debug)]
pub enum DownloadError {
    Network(reqwest::Error),
    ChecksumMismatch {
        expected: String,
        calculated: String,
    },
    NotFound,
    Stalled {
        bytes_received: usize,
    },
}

impl fmt::Display for DownloadError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            DownloadError::Network(err) => write!(f, "Errore di rete/HTTP: {err}"),
            DownloadError::ChecksumMismatch {
                expected,
                calculated,
            } => {
                write!(
                    f,
                    "Mismatch SHA-512! Atteso: {expected}, Calcolato: {calculated}"
                )
            }
            DownloadError::NotFound => write!(f, "File not found on server"),
            DownloadError::Stalled { bytes_received } => write!(
                f,
                "Download inactive for longer than timeout. Bytes received {bytes_received}"
            ),
        }
    }
}

impl std::error::Error for DownloadError {}

impl From<reqwest::Error> for DownloadError {
    fn from(err: reqwest::Error) -> Self {
        DownloadError::Network(err)
    }
}

/// Scarica un file in memoria e verifica l'hash SHA-512 rispetto a quello atteso.
/// Il download del file deve rimanere attivo per almento il tempo definito
/// nella costante CHUNK_INACTIVITY_TIMEOUT.
///
/// - `expected_sha512`: l'hash SHA-512 in formato esadecimale (da tlpdb)
pub async fn download_file(
    client: &Client,
    url: &str,
    expected_sha512: &str,
) -> Result<Bytes, DownloadError> {
    // Download asincrono dei byte via HTTP
    let response = client
        .get(url)
        .send()
        .await
        .map_err(DownloadError::Network)?
        .error_for_status()
        .map_err(|e| match e.status() {
            Some(status) if status == reqwest::StatusCode::NOT_FOUND => DownloadError::NotFound,
            _ => DownloadError::Network(e),
        })?;

    let mut stream = response.bytes_stream();
    let mut buf = BytesMut::new();

    loop {
        match timeout(config::network::CHUNK_INACTIVITY_TIMEOUT, stream.next()).await {
            // il chunk ricevuto in tempo resetta implicitamente il timer che al prossimo ciclo si riazzera
            // caso 1: ricevuto un chunk di dati dalla rete
            Ok(Some(Ok(chunk))) => {
                buf.extend_from_slice(&chunk);
            }
            // caso 2: errore di rete propagato da reqwest (connessione chiusa, reset, ecc.)
            Ok(Some(Err(e))) => {
                return Err(DownloadError::Network(e));
            }
            // caso 3: stream terminato correttamente (fine del body)
            Ok(None) => break,

            // caso 4: nessun chunk per CHUNK_INACTIVITY_TIMEOUT: il download è classificato inattivo.
            Err(_) => {
                return Err(DownloadError::Stalled {
                    bytes_received: buf.len(),
                });
            }
        }
    }

    let bytes = buf.freeze();

    // controllo SHA-512
    let mut hasher = Sha512::new();
    hasher.update(&bytes);
    let calculated_hash = hex::encode(hasher.finalize());

    // confronto case-insensitive. Gli hash nel tlpdb potrebbero essere minuscoli
    if !calculated_hash.eq_ignore_ascii_case(expected_sha512) {
        return Err(DownloadError::ChecksumMismatch {
            expected: expected_sha512.to_lowercase(),
            calculated: calculated_hash,
        });
    };

    Ok(bytes)
}

// stadio 0: avvio streaming dei file da scaricare
// stadio 1: download
// stadio 2: unpack tar extraction and routing
// stadio 3: I/O over disk

// decompressione: stadio 2
fn unpack_and_route(
    id_worker: usize,
    name: String,
    compressed_data: Bytes,
    relocated: bool,
    tx: &mpsc::Sender<WritePayload>,
) -> Result<(), String> {
    // directory di destinazione dei file
    let tlobj_dir = PathBuf::from("tlpkg");
    let texmf_dist_dir = PathBuf::from("texmf-dist");

    // Decompressione XZ in memoria tramite lzma-rs
    // Pre-alloca la RAM assumendo un rapporto di compressione ~4x
    let cursor = std::io::Cursor::new(&compressed_data);
    let estimated_capacity = compressed_data.len().saturating_mul(4);
    let mut decompressed_data = Vec::with_capacity(estimated_capacity);

    lzma_rs::xz_decompress(&mut std::io::BufReader::new(cursor), &mut decompressed_data)
        .map_err(|e| format!("[unpack stage] Errore decompressione XZ con lzma-rs: {e}"))?;
    info!("[XZ {id_worker}] '{name}' extracted");

    // lettura archivio tar
    let tar_cursor = std::io::Cursor::new(decompressed_data);
    let mut archive = Archive::new(tar_cursor);
    let entries = archive.entries().map_err(|e| {
        format!("[unpack stage] Errore lettura delle entry TAR: {e} per il file {name}")
    })?;

    // extract files from tar archive and route the file destination
    let name_str = name.as_str();
    let mut file_counter = 0_u32;
    for entry in entries {
        let mut entry = entry.map_err(|e| {
            format!("[unpack stage] Errore lettura voce TAR: {e} per il file {name_str}")
        })?;

        let rel_path = entry
            .path()
            .map_err(|e| {
                format!("[unpack stage] Errore nel path della voce TAR: {e} per il file {name_str}")
            })?
            .to_path_buf();
        let rel_path = sanitize_path(&rel_path);

        // Routing dei percorsi:
        //     tlobj -> tlobj_dir
        //     relocated -> texmf-dist
        let routed_path = match rel_path {
            p if p.starts_with("tlobj") => tlobj_dir.join(p),
            p if relocated => texmf_dist_dir.join(p),
            _ => rel_path,
        };

        let name = name.clone();
        match entry.header().entry_type() {
            EntryType::Directory => {
                tx.blocking_send(WritePayload::Directory { name, routed_path })
                    .map_err(|_| {
                        "[unpack] canale disk writer chiuso inaspettatamnete".to_string()
                    })?;
            }
            EntryType::Regular => {
                file_counter += 1;
                let size = entry.header().size()
                    .map_err(|e| format!("[unpack] impossibile convertire il campo size dell'entry tar {name_str}: {e}"))?
                as usize;
                let mut data = Vec::with_capacity(size);
                entry.read_to_end(&mut data).map_err(|e| {
                    format!("[unpack] errore nella lettora dati nel tar file {name_str}: {e}")
                })?;
                tx.blocking_send(WritePayload::File {
                    name,
                    routed_path,
                    data,
                })
                .map_err(|_| "[unpack] canale disk writer chiuso inaspettatamente".to_string())?;
            }
            _ => {
                eprintln!("[unpack] Trovato un elemento non gestito nell'archivio tar {name_str}");
            }
        }
    }
    info!("[XZ {id_worker}] job '{name}' sent to the disk writer {file_counter} files");
    Ok(())
}

/// Converte qualsiasi percorso (anche con separatori POSIX '/') in un PathBuf
/// nativo per l'OS corrente
fn sanitize_path(raw_path: &Path) -> PathBuf {
    let mut clean_path = PathBuf::new();
    for component in raw_path.components() {
        // normalizza ogni segmento eliminando gli slash nativi del TAR
        clean_path.push(component.as_os_str());
    }
    clean_path
}

// worker per il salvataggio su disco dei file
fn spawn_disk_writer(
    install_dir: PathBuf,
    mut rx: mpsc::Receiver<WritePayload>,
) -> JoinHandle<Result<(), String>> {
    tokio::task::spawn_blocking(move || {
        // costruisce l'albero dei file salvati
        // così posso tracciare le nuove directory da creare e scrivere ls-R velocemente
        let mut texlive_tree = tree::Node::default();

        while let Some(payload) = rx.blocking_recv() {
            match payload {
                WritePayload::File {
                    name,
                    routed_path,
                    data,
                } => {
                    // scrivi il file, aggiorna l'albero/ls-R
                    let new_dirs = texlive_tree.insert_file_path(&routed_path);
                    for dir in new_dirs.iter().map(|p| install_dir.join(p)) {
                        fs::create_dir(&dir).map_err(|e| {
                            format!("[spawn_disk_writer] Impossibile creare la cartella {:?}: {e} (archivio: {name})", dir)
                        })?;
                    }

                    let destination = install_dir.join(routed_path);
                    let size = data.len() as u64;

                    let mut file = File::create(&destination).map_err(|e| {
                        format!(
                            "Errore creazione file {:?}: {e} (archivio: {name})",
                            destination
                        )
                    })?;

                    // Prealloca solo per file di dimensioni rilevanti
                    if size >= 65536 {
                        file.set_len(size).map_err(|e| {
                            format!("Errore di impostazione della lunghezza del file: {e}")
                        })?;
                    }

                    file.write_all(&data).map_err(|e| {
                        format!(
                            "Errore di scrittura del file {:?}: {e} (archivio: {name})",
                            destination
                        )
                    })?;
                }
                WritePayload::Directory { name, routed_path } => {
                    let new_dirs = texlive_tree.insert_dir(&routed_path);
                    for dir in new_dirs.iter().map(|p| install_dir.join(p)) {
                        fs::create_dir(&dir).map_err(|e| {
                            format!("[spawn_disk_writer] Impossibile creare la cartella {:?}: {e} (archivio: {name})", dir)
                        })?;
                    }
                }
            }
        }

        // generazione ls-R per la directory texmf-dist
        // Buffer personalizzato da 64 KB (64 * 1024 byte)
        let lsr_path = install_dir.join("texmf-dist").join("ls-R");
        let file = std::fs::File::create(&lsr_path).map_err(|e| {
            format!(
                "Errore nella creazione del file ls-R in {:?}: {e}",
                &lsr_path
            )
        })?;
        let mut writer = std::io::BufWriter::with_capacity(64 * 1024, file);

        // get the right node of texmf-dist directory
        let Some(tnode) = texlive_tree.dirs.get("texmf-dist") else {
            unreachable!(
                "[create ls-R for texmf-dist] la directory texmf-dist non è stata trovata nell'oggetto tree"
            )
        };

        tnode.generate_texmf_dist_ls_r(&mut writer).map_err(|e| {
            format!(
                "Errore nella scrittura del file ls-R in {:?}: {e}",
                &lsr_path
            )
        })?;

        Ok(())
    })
}
