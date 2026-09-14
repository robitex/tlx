// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at http://mozilla.org/MPL/2.0/.

// SPDX-License-Identifier: MPL-2.0

use crate::database::arch::is_known_tl_arch;
use Entry::*;
use std::io::Write;

use crate::execute::addformat::AddFormat;
use crate::execute::addhyphen::AddHyphen;

use crate::networking::pipeline::RemoteFile;

/// strutture dati di rappresentazione dei dataset dei pacchetti TeX Live

/// parser di file texlive.tlpdb
pub struct Parser;

/// an element of information of a meta-package
/// 'name' is a field of Package struct
#[derive(Debug)]
enum Entry<'a> {
    // general information:
    Category(&'a str),
    Revision(u32),
    Catalogue(&'a str),
    Shortdesc(&'a str),
    Longdesc(Items<'a>),
    Relocated,

    // depend
    Depend(Items<'a>),

    // action
    Postaction(Items<'a>),
    Execute(Items<'a>),

    // container
    RunContainer(Container<'a>),
    DocContainer(Container<'a>),
    SrcContainer(Container<'a>),

    RunFiles((u64, Items<'a>)), // file list: RunFiles((size_kb, files))
    BinFiles((&'a str, u64, Items<'a>)), // file list: BinFiles( (arch, size_kb, files) )
    DocFiles((u64, Items<'a>)), // file list: DocFiles((size_kb, files))
    SrcFiles((u64, Items<'a>)), // file list: SrcFiles((size_kb, files))

    // tails catalogue information
    CatalogueEntry((&'a str, &'a str)),
}

/// retain values when consecutive lines has the format 'key value\nkey value\nkey value ...'
/// the list of files is parsed from the database texlive.tlpdb (example: runfiles, docfiles, etc)
#[derive(Debug)]
struct Items<'a> {
    prefix: &'a str,
    list: Vec<&'a str>,
}

/// correspond to a downloadable file of the collection of TeX Live packages
#[derive(Debug)]
struct Container<'a> {
    byte_size: u64,    // Dimensione in byte del file da scaricare
    checksum: &'a str, // SHA-512 checksum of the file that will be downloaded from CTAN
}

/// package is a dataset representing a unit component in the TeX Live infrastructure
#[derive(Debug)]
pub struct Package<'a> {
    name: &'a str,
    revision: u32,
    relocated: bool,
    //
    dataset: Vec<Entry<'a>>,
    //
    index_depend: Option<usize>,        // index of Depend() Entry
    index_execute: Option<usize>,       // index of execute directives
    index_post_action: Option<usize>,   // index of postaction directives
    index_container: Option<usize>,     // index of container data
    index_doc_container: Option<usize>, // index of doccontainer data
    index_src_container: Option<usize>, // index of srccontainer data
}

impl<'a> Items<'a> {
    fn parse_files_from_iter<I>(iter: &mut I) -> (usize, Self)
    where
        I: Iterator<Item = &'a str> + Clone,
    {
        // Cloniamo l'iteratore: costa ZERO allocazioni (copia solo i puntatori)
        let explorer = iter.clone();
        let tot_capacity = explorer.take_while(|line| line.starts_with(" ")).count();

        // ALLOCAZIONE ESATTA del vettore
        let mut items = Vec::with_capacity(tot_capacity);

        // LETTURA DATI
        // Avanziamo l'iteratore reale per il numero esatto di righe contate
        for _ in 0..tot_capacity {
            let line = iter
                .next()
                .expect("Errore: riga attesa durante la lettura del blocco dati");
            let val = line.strip_prefix(" ").expect(
                "Errore: prefisso '{prefix}' atteso durante la lettura della linea '{line}'",
            );

            items.push(val);
        }
        debug_assert!(
            tot_capacity == items.len(),
            "{tot_capacity} :: len -> {}",
            items.len()
        );
        (
            tot_capacity + 1,
            Self {
                prefix: " ",
                list: items,
            },
        )
    }

    fn parse_keyval_from_iter<I>(
        iter: &mut I,
        prefix: &'a str,
        first_value: &'a str,
    ) -> (usize, Self)
    where
        I: Iterator<Item = &'a str> + Clone,
    {
        // Cloniamo l'iteratore: costa ZERO allocazioni (copia solo i puntatori)
        let explorer = iter.clone();
        let tot_capacity = explorer.take_while(|line| line.starts_with(prefix)).count() + 1;

        // ALLOCAZIONE ESATTA del vettore
        let mut items = Vec::with_capacity(tot_capacity);
        items.push(first_value);

        // LETTURA DATI
        // Avanziamo l'iteratore reale per il numero esatto di righe contate
        for _ in 0..tot_capacity - 1 {
            let line = iter
                .next()
                .expect("Errore: riga attesa durante la lettura del blocco dati");
            let val = line.strip_prefix(prefix).expect(
                "Errore: prefisso '{prefix}' atteso durante la lettura della linea '{line}'",
            );

            items.push(val);
        }
        debug_assert!(
            tot_capacity == items.len(),
            "forecast -> {tot_capacity} :: len -> {}",
            items.len()
        );
        (
            tot_capacity,
            Self {
                prefix,
                list: items,
            },
        )
    }

    // funzione di scrittura su buffer
    fn write_to<W: Write>(&self, writer: &mut W) -> std::io::Result<()> {
        let prefix = self.prefix.as_bytes();
        for &elem in self.list.iter() {
            writer.write_all(prefix)?;
            if let Some(reloc_path) = elem.strip_prefix("RELOC") {
                writer.write_all(b"texmf-dist")?;
                writer.write_all(reloc_path.as_bytes())?;
            } else {
                writer.write_all(elem.as_bytes())?;
            }
            writer.write_all(b"\n")?;
        }
        Ok(())
    }
}

impl<'a> Container<'a> {
    /// Scrive i dati della struct in un writer (es. `BufWriter`),
    /// a Zero-Allocazioni e senza formattazione dinamica.
    fn write_to<W: Write>(&self, writer: &mut W, prefix: Option<&str>) -> std::io::Result<()> {
        // Scrive: <prefix>size <size>\n
        if let Some(pf) = prefix {
            writer.write_all(pf.as_bytes())?;
        }
        writer.write_all(b"containersize ")?;
        write!(writer, "{}", self.byte_size)?;
        writer.write_all(b"\n")?;

        // Scrive: <prefix>checksum <checksum>\n
        if let Some(pf) = prefix {
            writer.write_all(pf.as_bytes())?;
        }
        writer.write_all(b"containerchecksum ")?;
        writer.write_all(self.checksum.as_bytes())?;
        writer.write_all(b"\n")?;
        Ok(())
    }
}

impl<'a> Entry<'a> {
    #[inline]
    fn write_str_str<W: Write>(writer: &mut W, key: &[u8], val: &[u8]) -> std::io::Result<()> {
        writer.write_all(key)?;
        writer.write_all(b" ")?;
        writer.write_all(val)?;
        writer.write_all(b"\n")
    }
    #[inline]
    fn write_str_u32<W: Write>(writer: &mut W, key: &[u8], val: &u32) -> std::io::Result<()> {
        writer.write_all(key)?;
        writer.write_all(b" ")?;
        write!(writer, "{val}\n")
    }
    #[inline]
    fn write_str_size<W: Write>(writer: &mut W, key: &[u8], size: &u64) -> std::io::Result<()> {
        writer.write_all(key)?;
        write!(writer, " size={size}\n")
    }
    #[inline]
    fn write_str_arch_size<W: Write>(
        writer: &mut W,
        key: &[u8],
        arch: &[u8],
        size: &u64,
    ) -> std::io::Result<()> {
        writer.write_all(key)?;
        writer.write_all(b" arch=")?;
        writer.write_all(arch)?;
        write!(writer, " size={size}\n")
    }

    // funzione di scrittura su buffer
    fn write_to<W: Write>(&self, writer: &mut W, is_scheme: bool) -> std::io::Result<()> {
        match self {
            Category(category) => Self::write_str_str(writer, b"category", category.as_bytes())?,
            Revision(rev) => Self::write_str_u32(writer, b"revision", rev)?,
            Catalogue(cat) => Self::write_str_str(writer, b"catalogue", cat.as_bytes())?,
            Shortdesc(desc) => Self::write_str_str(writer, b"shortdesc", desc.as_bytes())?,
            Longdesc(rows) => rows.write_to(writer)?,
            Relocated => if is_scheme { writer.write_all(b"relocated 1\n")? }, // relocated package must be localized and marked as a normal package
            Depend(rows) => rows.write_to(writer)?,
            Postaction(rows) => rows.write_to(writer)?,
            Execute(rows) => rows.write_to(writer)?,
            RunContainer(cont) => cont.write_to(writer, None)?,
            DocContainer(cont) => cont.write_to(writer, Some("doc"))?,
            SrcContainer(cont) => cont.write_to(writer, Some("src"))?,
            RunFiles((size, items)) => {
                Self::write_str_size(writer, b"runfiles", size)?;
                items.write_to(writer)?;
            }
            BinFiles((arch, size, items)) => {
                Self::write_str_arch_size(writer, b"binfiles", arch.as_bytes(), size)?;
                items.write_to(writer)?;
            }
            DocFiles((size, items)) => {
                Self::write_str_size(writer, b"docfiles", size)?;
                items.write_to(writer)?;
            }
            SrcFiles((size, items)) => {
                Self::write_str_size(writer, b"srcfiles", size)?;
                items.write_to(writer)?;
            }
            CatalogueEntry((entry, info)) => {
                Self::write_str_str(writer, entry.as_bytes(), info.as_bytes())?
            }
        };
        Ok(())
    }
}

impl<'a> Package<'a> {
    // examples of files name
    // 12many.r79618.tar.xz
    // 12many.doc.r79618.tar.xz
    // 12many.source.r79618.tar.xz

    fn archive_spec(&self) -> Option<RemoteFile> {
        if let Some(index) = self.index_container {
            let Some(entry) = self.dataset.get(index) else {
                unreachable!("Logic error: index {index} out of bounds for dataset");
            };
            let RunContainer(container) = entry else {
                let name = self.name;
                unreachable!("Logic error: index {index} in not a container of {name} package");
            };
            Some(RemoteFile::new(
                format!("{}.r{}.tar.xz", self.name, self.revision),
                container.checksum.to_string(),
                self.relocated,
            ))
        } else {
            None
        }
    }

    fn archive_doc_spec(&self) -> Option<RemoteFile> {
        if let Some(index) = self.index_doc_container {
            let Some(entry) = self.dataset.get(index) else {
                unreachable!("Logic error: index {index} out of bounds for dataset");
            };
            let DocContainer(doc_container) = entry else {
                let name = self.name;
                unreachable!(
                    "Logic error: index {index} in not a document container of {name} package"
                );
            };
            Some(RemoteFile::new(
                format!("{}.doc.r{}.tar.xz", self.name, self.revision),
                doc_container.checksum.to_string(),
                self.relocated,
            ))
        } else {
            None
        }
    }

    fn archive_src_spec(&self) -> Option<RemoteFile> {
        if let Some(index) = self.index_src_container {
            let Some(entry) = self.dataset.get(index) else {
                unreachable!("Logic error: index {index} out of bounds for dataset");
            };
            let SrcContainer(src_container) = entry else {
                let name = self.name;
                unreachable!(
                    "Logic error: index {index} in not a source container of {name} package"
                );
            };
            Some(RemoteFile::new(
                format!("{}.source.r{}.tar.xz", self.name, self.revision),
                src_container.checksum.to_string(),
                self.relocated,
            ))
        } else {
            None
        }
    }

    /// if any, return the slice with every dependencies of the package
    /// .ARCH suffix is converted in .<target_arch>
    pub fn dependencies_as_slice(&self) -> Option<&[&'a str]> {
        // retrive package's dependencies
        if let Some(index) = self.index_depend {
            let Some(entry) = self.dataset.get(index) else {
                unreachable!("Logic error: index {index} out of bounds for dataset");
            };
            let Depend(items) = entry else {
                unreachable!(
                    "Logic error: se esiste il campo index_depend \
                    deve esistere anche Entry::Depend"
                );
            };
            Some(items.list.as_slice())
        } else {
            None
        }
    }

    #[allow(dead_code)]
    pub fn execute_as_slice(&self) -> Option<&[&'a str]> {
        if let Some(index) = self.index_execute {
            let Some(entry) = self.dataset.get(index) else {
                unreachable!("Logic error: index {index} out of bounds for dataset");
            };
            let Execute(execute_list) = entry else {
                let name = self.name;
                unreachable!(
                    "Logic error: index {index} in not a 'execute' list of {name} package"
                );
            };
            Some(execute_list.list.as_slice())
        } else {
            None
        }
    }

    pub fn get_add_format_directives(&self) -> Vec<AddFormat<'a>> {
        let mut addformat = Vec::new();
        if let Some(index) = self.index_execute {
            let Some(entry) = self.dataset.get(index) else {
                unreachable!("Logic error: index {index} out of bounds for dataset")
            };
            let Execute(execute_list) = entry else {
                let name = self.name;
                unreachable!(
                    "Logic error: index {index} in not a 'execute' list of {name} package"
                );
            };
            for &exec_str in &execute_list.list {
                if exec_str.starts_with("AddFormat") {
                    if let Some(fmt) = AddFormat::parse(self.name, exec_str.trim()) {
                        addformat.push(fmt);
                    }
                }
            }
        }
        addformat
    }

    pub fn get_hyphen_directives(&self) -> Vec<AddHyphen<'a>> {
        let mut addhyphen = Vec::new();

        if let Some(index) = self.index_execute {
            let Some(entry) = self.dataset.get(index) else {
                unreachable!("Logic error: index {index} out of bounds for dataset");
            };
            let Execute(execute_list) = entry else {
                let name = self.name;
                unreachable!(
                    "Logic error: index {index} in not a 'execute' list of {name} package"
                );
            };
            for &exec_str in &execute_list.list {
                if exec_str.starts_with("AddHyphen") {
                    if let Some(h) = AddHyphen::parse(self.name, exec_str.trim()) {
                        addhyphen.push(h);
                    }
                }
            }
        }
        addhyphen
    }

    #[allow(dead_code)]
    pub fn postaction_as_slice(&self) -> Option<&[&'a str]> {
        if let Some(index) = self.index_post_action {
            let Some(entry) = self.dataset.get(index) else {
                unreachable!("Logic error: index {index} out of bounds for dataset");
            };
            let Postaction(postaction_list) = entry else {
                let name = self.name;
                unreachable!(
                    "Logic error: index {index} in not a 'postaction' list of {name} package"
                );
            };
            Some(postaction_list.list.as_slice())
        } else {
            None
        }
    }

    // create am empty meta-package
    fn new(name: &'a str) -> Package<'a> {
        Package {
            name,
            revision: 0,
            relocated: false,
            dataset: Vec::with_capacity(28), // max elements registered 24
            // main element indexing
            index_depend: None,
            index_execute: None,
            index_post_action: None,
            index_container: None,
            index_doc_container: None,
            index_src_container: None,
        }
    }

    // add an entry
    fn insert(&mut self, item: Entry<'a>) {
        // save the index: an implentation detail
        match item {
            Depend(_) => self.index_depend = Some(self.dataset.len()),
            Execute(_) => self.index_execute = Some(self.dataset.len()),
            Postaction(_) => self.index_post_action = Some(self.dataset.len()),
            RunContainer(_) => self.index_container = Some(self.dataset.len()),
            DocContainer(_) => self.index_doc_container = Some(self.dataset.len()),
            SrcContainer(_) => self.index_src_container = Some(self.dataset.len()),
            _ => {}
        };

        self.dataset.push(item);
    }

    // retrive package name
    pub fn get_name(&self) -> &'a str {
        self.name
    }

    // build a new package from a block of lines of the .tlpdb files
    pub fn parse_from_str(
        block: &'a str,
        target_arch: &'a str,
        #[allow(unused_variables)] include_doc: bool,
        #[allow(unused_variables)] include_src: bool,
        skip_filter: bool,
    ) -> Option<Package<'a>> {
        // eliminate in production
        let lines_count = block.lines().count();
        // eliminate in production

        // main iterator over lines
        let mut iter = block.lines();

        // the first three line must be define the package's name, category and revision
        let name = iter
            .next()
            .expect("attesa almeno una riga")
            .strip_prefix("name ")
            .expect("la prima riga non ha la chiave 'name'");

        // package filters
        if skip_filter && Package::should_skip_package(name, target_arch) {
            return None;
        }

        // the ordered list of package data
        let mut pkg = Package::new(name);

        // category
        let category = iter
            .next()
            .expect("attesa una seconda linea")
            .strip_prefix("category ")
            .expect("la seconda riga non ha la chiave 'category': pacchetto {name}");
        pkg.insert(Category(category));

        let revision = iter
            .next()
            .expect("attesa una terza linea")
            .strip_prefix("revision ")
            .expect("la terza riga non ha la chiave 'revision': pacchetto {field_01_name}")
            .parse()
            .expect("atteso un intero per il campo 'revision': pacchetto {field_01_name}");

        pkg.insert(Revision(revision));
        // temporary solution:
        pkg.revision = revision;

        let mut line_processed = 3_usize;

        // cycled parsing
        while let Some(line) = iter.next() {
            if let Some((key, value)) = line.split_once(' ') {
                match key {
                    "catalogue" => {
                        pkg.insert(Catalogue(value));
                        line_processed += 1;
                    }
                    "shortdesc" => {
                        pkg.insert(Shortdesc(value));
                        line_processed += 1;
                    }
                    "relocated" => {
                        // asserzione: panic se il valore che segue il campo ''
                        debug_assert!(
                            value == "1",
                            "Errore: direttiva 'relocated' non è 1 per il pacchetto '{name}'!"
                        );
                        pkg.insert(Relocated);
                        pkg.relocated = true;
                        line_processed += 1;
                    }
                    "longdesc" => {
                        let (line_count, items) =
                            Items::parse_keyval_from_iter(&mut iter, "longdesc ", value);
                        pkg.insert(Longdesc(items));
                        line_processed += line_count;
                    }
                    "postaction" => {
                        let (line_count, items) =
                            Items::parse_keyval_from_iter(&mut iter, "postaction ", value);
                        pkg.insert(Postaction(items));
                        line_processed += line_count;
                    }
                    "depend" => {
                        let (line_count, items) =
                            Items::parse_keyval_from_iter(&mut iter, "depend ", value);
                        pkg.insert(Depend(items));
                        line_processed += line_count;
                    }
                    "execute" => {
                        let (line_count, items) =
                            Items::parse_keyval_from_iter(&mut iter, "execute ", value);
                        pkg.insert(Execute(items));
                        line_processed += line_count;
                    }
                    "containersize" => {
                        let byte_size = value
                            .parse()
                            .expect("Errore: atteso numero per la misura di 'containersize'. Pacchetto '{name}'");
                        let checksum = iter
                            .next()
                            .and_then(|line| line.strip_prefix("containerchecksum "))
                            .expect("Errore: riga 'srccontainerchecksum' mancante o malformata");
                        pkg.insert(RunContainer(Container {
                            byte_size,
                            checksum,
                        }));
                        line_processed += 2;
                    }
                    "doccontainersize" => {
                        let byte_size = value.parse().expect(
                            "Errore: attesa misura di 'doccontainersize'. Pacchetto '{name}'",
                        );
                        let checksum = iter
                            .next()
                            .and_then(|line| line.strip_prefix("doccontainerchecksum "))
                            .expect("Errore: riga 'doccontainerchecksum' mancante o malformata");
                        pkg.insert(DocContainer(Container {
                            byte_size,
                            checksum,
                        }));
                        line_processed += 2;
                    }
                    "srccontainersize" => {
                        let byte_size = value.parse().expect(
                            "Errore: attesa misura di 'srccontainersize'. Pacchetto '{name}'",
                        );
                        let checksum = iter
                            .next()
                            .and_then(|line| line.strip_prefix("srccontainerchecksum "))
                            .expect("Errore: riga 'srccontainerchecksum' mancante o malformata");
                        pkg.insert(SrcContainer(Container {
                            byte_size,
                            checksum,
                        }));
                        line_processed += 2;
                    }
                    "runfiles" => {
                        let size = value
                            .strip_prefix("size=")
                            .expect("atteso campo 'size=' per runfiles (pacchetto '{name}')")
                            .parse()
                            .expect("valore misura malformato per runfiles in '{value}' (pacchetto {name})");

                        let (line_count, items) = Items::parse_files_from_iter(&mut iter);
                        pkg.insert(RunFiles((size, items)));
                        line_processed += line_count;
                    }
                    "binfiles" => {
                        let (arch, size): (&str, u64) = match value.split_once(' ') {
                            Some((arch, size)) => {(
                                arch
                                .strip_prefix("arch=")
                                .expect("Manca il prefisso 'arch=' per il pacchetto '{name}'"),
                                size
                                .strip_prefix("size=")
                                .expect("Manca il prefisso 'size='")
                                .parse()
                                .expect("Valore numerico non valido per 'size=' per il pacchetto '{name}")
                            )},
                            None => panic!("Formato non valido: previsti 'arch=' e 'size=' separati da spazio in: '{value}'"),
                        };

                        let (line_count, items) = Items::parse_files_from_iter(&mut iter);
                        pkg.insert(BinFiles((arch, size, items)));
                        line_processed += line_count;
                    }
                    "docfiles" => {
                        let size = value
                            .strip_prefix("size=")
                            .expect("atteso campo 'size=' per docfiles (pacchetto '{name}')")
                            .parse()
                            .expect("valore misura malformato per docfiles in '{value}' (pacchetto {name})");

                        let (line_count, items) = Items::parse_files_from_iter(&mut iter);
                        pkg.insert(DocFiles((size, items)));
                        line_processed += line_count;
                    }
                    "srcfiles" => {
                        let size = value
                            .strip_prefix("size=")
                            .expect("atteso campo 'size=' per srcfiles (pacchetto '{name}')")
                            .parse()
                            .expect("valore misura malformato per srcfiles in '{value}' (pacchetto {name})");

                        let (line_count, items) = Items::parse_files_from_iter(&mut iter);
                        pkg.insert(SrcFiles((size, items)));
                        line_processed += line_count;
                    }
                    other => {
                        debug_assert!(
                            other.starts_with("catalogue-"),
                            "campo '{other}' non gestito alla inea '{line}' (pacchetto '{name}')"
                        );
                        pkg.insert(CatalogueEntry((other, value)));
                        line_processed += 1;

                        // totally consume the iterator
                        while let Some(line) = iter.next() {
                            if let Some((key, val)) = line.split_once(' ') {
                                debug_assert!(
                                    key.starts_with("catalogue-"),
                                    "campo '{other}' non gestito alla inea '{line}' (pacchetto '{name}')"
                                );
                                pkg.insert(CatalogueEntry((key, val)));
                                line_processed += 1;
                            }
                        }
                    }
                }; // end 'match key'
            }; // end 'if let'
        } // end 'while let'

        debug_assert!(
            line_processed == lines_count,
            "ERR: meta-package {name}: processate: {line_processed} vs blocco: {lines_count}"
        );
        Some(pkg)
    }

    // scrive in un buffer il dataset del meta-pacchetto secondo il formato originale
    pub fn write_to<W: Write>(&self, writer: &mut W) -> std::io::Result<()> {
        // print the package name
        writer.write_all(b"name ")?;
        writer.write_all(self.name.as_bytes())?;
        writer.write_all(b"\n")?;
        // print remaining directives and fields
        let is_scheme = self.get_name().starts_with("scheme-");
        for entry_set in self.dataset.iter() {
            entry_set.write_to(writer, is_scheme)?;
        }
        Ok(())
    }

    /// Ritorna `true` se il nome del pacchetto appartiene a un'architettura che vogliamo ignorare.
    #[inline]
    fn should_skip_package(pkg_name: &str, target_arch: &str) -> bool {
        // 1. I pacchetti binari di TeX Live finiscono tipicamente con ".<architettura>"
        if let Some((_, pkg_arch)) = pkg_name.rsplit_once('.') {
            // Se c'è un'estensione/architettura nel nome (es. "x86_64-linux")
            // e NON è l'architettura target, la saltiamo.
            if is_known_tl_arch(pkg_arch) && pkg_arch != target_arch {
                return true;
            }
        }

        false
    }

    /// Helper per estrarre tutti i file validi del pacchetto
    pub fn remote_files(&self) -> impl Iterator<Item = RemoteFile> {
        [
            self.archive_spec(),
            self.archive_doc_spec(),
            self.archive_src_spec(),
        ]
        .into_iter()
        .flatten()
    }
}

impl Parser {
    pub const PKG_COUNT: usize = 9000;
}

// elenco schemi:
//
// Schema: scheme-context
// Schema: scheme-minimal
// Schema: scheme-gust
// Schema: scheme-infraonly
// Schema: scheme-basic
// Schema: scheme-bookpub
// Schema: scheme-small
// Schema: scheme-medium
// Schema: scheme-tetex
// Schema: scheme-full
