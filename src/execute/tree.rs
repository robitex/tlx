// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at http://mozilla.org/MPL/2.0/.

// SPDX-License-Identifier: MPL-2.0

///! costruzione della struttura ad albero dei file allo scopo di gestire
///! sia la creazione dei file ls-R sia la creazione delle directory
///! durante la copia su disco dei file tratti dagli archivi tar
use ahash::AHashMap;

use std::fs;
use std::io::Write;
use std::path::Path;
use std::path::PathBuf;

/// Nodo dell'albero per rappresentare la gerarchia dell'albero di TeX Live
pub struct Node {
    pub dirs: AHashMap<String, Node>,
    pub files: Vec<String>,
}

impl Default for Node {
    fn default() -> Self {
        Node {
            dirs: AHashMap::with_capacity(16),
            files: Vec::with_capacity(64),
        }
    }
}

impl Node {
    /// Inserisce il percorso del file nell'albero.
    /// Restituisce la lista di sub-percorsi di directory che sono stati creati EX NOVO nell'albero.
    pub fn insert_file_path(&mut self, rel_path: &Path) -> Vec<PathBuf> {
        let mut resulting_new_dirs = Vec::new();
        let mut current_node = self;
        let mut current_path = PathBuf::new();

        if let Some(parent) = rel_path.parent() {
            for component in parent.components() {
                let comp_name = component.as_os_str();
                let Some(comp_name_str) = comp_name.to_str() else {
                    panic!(
                        "[Node::insert_file_path] component {:?} in not a valid ascii string",
                        comp_name.to_string_lossy()
                    );
                };

                current_path.push(comp_name);

                // Verifichiamo se il ramo esiste già nell'albero
                let is_new = !current_node.dirs.contains_key(comp_name_str);

                current_node = current_node
                    .dirs
                    .entry(comp_name_str.to_string())
                    .or_default();

                if is_new {
                    // Notifichiamo all'esterno che questa specifica directory è nuova per l'albero
                    resulting_new_dirs.push(current_path.clone());
                };
            }
        };

        // Inseriamo il file nel nodo foglia per ls-R
        if let Some(file_name) = rel_path.file_name() {
            let Some(file_name_str) = file_name.to_str() else {
                panic!(
                    "[Node::insert_file_path] filename {:?} in not a valid ascii string",
                    file_name.to_string_lossy()
                );
            };
            current_node.files.push(file_name_str.to_string());
        }

        resulting_new_dirs
    }

    pub fn insert_dir(&mut self, rel_path: &Path) -> Vec<PathBuf> {
        let mut resulting_new_dirs = Vec::new();
        let mut current_node = self;
        let mut current_path = PathBuf::new();

        for component in rel_path.components() {
            let comp_name = component.as_os_str();
            let Some(comp_name_str) = comp_name.to_str() else {
                panic!(
                    "[Node::insert_file_path] component {:?} in not a valid ascii string",
                    comp_name.to_string_lossy()
                );
            };

            current_path.push(comp_name);

            // Verifichiamo se il ramo esiste già nell'albero
            let is_new = !current_node.dirs.contains_key(comp_name_str);

            current_node = current_node
                .dirs
                .entry(comp_name_str.to_string())
                .or_default();

            if is_new {
                // Notifichiamo all'esterno che questa specifica directory è nuova per l'albero
                resulting_new_dirs.push(current_path.clone());
            }
        }

        resulting_new_dirs
    }

    /// Genera il contenuto ls-R in streaming su un buffer di scrittura.
    pub fn generate_texmf_dist_ls_r<W: Write>(&self, writer: &mut W) -> std::io::Result<()> {
        // Intestazione standard Kpathsea
        writer
            .write_all(b"% ls-R -- filename database for kpathsea; do not change this line.\n\n")?;

        let mut string_buf_for_path = String::with_capacity(256);
        string_buf_for_path.push('.');

        self.generate_ls_r_internal(&mut string_buf_for_path, writer, true)
    }

    fn generate_ls_r_internal<W: Write>(
        &self,
        path_buf: &mut String,
        writer: &mut W,
        is_root: bool,
    ) -> std::io::Result<()> {
        // raccolta di tutti gli items (file + cartelle)
        let mut tot_items = self.files.len() + self.dirs.len();
        if is_root {
            tot_items += 1;
        }
        let mut items: Vec<&str> = Vec::with_capacity(tot_items);

        if is_root {
            items.push("ls-R"); // Il file ls-R appartiene alla radice
        }

        items.extend(self.files.iter().map(|s| s.as_str()));
        let mut dirs: Vec<&str> = self.dirs.keys().map(|s| s.as_str()).collect();
        items.extend(dirs.iter());

        // Se non ci sono elementi, non stampiamo il blocco
        if items.is_empty() {
            return Ok(());
        }

        // ordinamento per la fusione locale dei contenuti
        items.sort_unstable();
        if is_root {
            items.dedup(); // protezione contro i duplicati ls-R
        }
        // Estraiamo le sotto-directory e le ordiniamo alfabeticamente per la discesa
        dirs.sort_unstable();

        // Scrittura del blocco su disco
        if is_root {
            writeln!(writer, "./:")?;
        } else {
            writeln!(writer, "{path_buf}:")?;
        };

        for item in items {
            writeln!(writer, "{item}")?;
        }
        writeln!(writer)?;

        let original_len = path_buf.len();

        // ricorsione ordinata senza allocazioni di stringhe
        for dir in dirs {
            let child_node = &self.dirs[dir];
            path_buf.push('/');
            path_buf.push_str(dir);
            child_node.generate_ls_r_internal(path_buf, writer, false)?;
            path_buf.truncate(original_len);
        }

        Ok(())
    }

    /// Scansiona ricorsivamente una directory sul filesystem reale e costruisce l'albero.
    pub fn from_dir(dir_path: &Path) -> std::io::Result<Self> {
        let mut node = Node::default();

        // Leggiamo il contenuto della cartella corrente
        let entries = fs::read_dir(dir_path)?;

        for entry in entries {
            let entry = entry?;
            let file_type = entry.file_type()?;
            let os_name = entry.file_name();
            let name_str = os_name.to_str().unwrap();

            if file_type.is_dir() {
                // È una sottocartella: scansioniamo ricorsivamente e la inseriamo in `dirs`
                let child_node = Self::from_dir(&entry.path())?;
                node.dirs.insert(name_str.to_string(), child_node);
            } else if file_type.is_file() {
                // È un file: lo inseriamo nel `Vec` dei file del nodo corrente
                node.files.push(name_str.to_string());
            }
            // I link simbolici o tipi speciali vengono ignorati,
            // ma se servono si possono gestire estendendo i rami if.
        }

        Ok(node)
    }
}
