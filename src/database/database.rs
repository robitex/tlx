// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at http://mozilla.org/MPL/2.0/.

// SPDX-License-Identifier: MPL-2.0

use ahash::AHashMap;
use ahash::AHashSet;

use std::io::Write;
use std::time::Instant;

use crate::database::package::Package;
use crate::database::package::Parser;
use crate::execute::addformat::AddFormat;
use crate::execute::addhyphen::AddHyphen;
use crate::networking::installer::RemoteFile;

pub struct Database<'a> {
    packages: Vec<Package<'a>>,
    // pkg_name -> index position of corresponding Package in the vec object
    index: AHashMap<&'a str, usize>,
    target_arch: &'static str,
    pub options: ConfigData<'a>,
    mirror_url: &'a str,
    include_doc: bool,
    include_src: bool,
}

impl<'a> Database<'a> {
    pub fn hyphen_directives_iter(
        &self,
        pkg_to_download: &AHashSet<&str>,
    ) -> impl Iterator<Item = AddHyphen<'_>> {
        self.packages
            .iter()
            .filter(|p| pkg_to_download.contains(p.get_name()))
            .flat_map(|p| p.get_hyphen_directives().into_iter())
    }

    pub fn format_directives_iter(
        &self,
        pkg_to_download: &AHashSet<&str>,
    ) -> impl Iterator<Item = AddFormat<'_>> {
        self.packages
            .iter()
            .filter(|p| pkg_to_download.contains(p.get_name()))
            .flat_map(|p| p.get_add_format_directives().into_iter())
    }

    pub fn write_tlpdb<W: Write>(
        &self,
        writer: &mut W,
        installed_package: &AHashSet<&str>,
    ) -> std::io::Result<()> {
        let opts = &self.options;
        // write 00texlive.config meta/virtual package
        writeln!(
            writer,
            "name 00texlive.config
category TLCore
depend minrelease/{}
depend release/{}\n",
            opts.min_release, opts.release
        )?;

        // write 00texlive.installation meta/virtual package
        writer.write_all(b"name 00texlive.installation\ncategory TLCore\n")?;
        // forced installation parameters waiting for a new design module
        writeln!(writer, "depend opt_autobackup:{}", opts.opt_autobackup)?;
        writeln!(writer, "depend opt_backupdir:{}", opts.opt_backupdir)?;
        writeln!(
            writer,
            "depend opt_create_formats:{}",
            if opts.opt_create_formats { "1" } else { "0" }
        )?;
        writeln!(writer, "depend opt_desktop_integration:{}", "0")?; // forced
        writeln!(writer, "depend opt_file_assocs:{}", "0")?; // forced
        writeln!(
            writer,
            "depend opt_generate_updmap:{}",
            if opts.opt_generate_updmap { "1" } else { "0" }
        )?;
        writeln!(
            writer,
            "depend opt_install_docfiles:{}",
            if self.include_doc { "1" } else { "0" }
        )?;
        writeln!(
            writer,
            "depend opt_install_srcfiles:{}",
            if self.include_src { "1" } else { "0" }
        )?;

        let url = self.mirror_url.trim_end_matches('/');
        writeln!(writer, "depend opt_location:{}", url)?;

        writeln!(writer, "depend opt_post_code:{}", "0")?; // forced
        writeln!(writer, "depend opt_sys_bin:{}", opts.opt_sys_bin)?;
        writeln!(writer, "depend opt_sys_info:{}", opts.opt_sys_info)?;
        writeln!(writer, "depend opt_sys_man:{}", opts.opt_sys_man)?;
        writeln!(writer, "depend opt_w32_multi_user:{}", "0")?; // forced
        writeln!(
            writer,
            "depend setting_available_architectures:{}\n",
            self.target_arch
        )?;

        let schemes = [
            "scheme-context",
            "scheme-bookpub",
            "scheme-tetex",
            "scheme-minimal",
            "scheme-basic",
            "scheme-medium",
            "scheme-gust",
            "scheme-small",
            "scheme-infraonly",
        ];

        for pkg in &self.packages {
            let name = pkg.get_name();
            if installed_package.contains(name) || schemes.contains(&name) {
                pkg.write_to(writer)?;
                writer.write_all(b"\n")?;
            }
        }
        writer.write_all(b"\n")?;
        writer.flush()?;
        Ok(())
    }

    // iteratore sui RemoteFile da installare
    pub fn remote_files_iter(
        &self,
        pkg_to_download: &ahash::AHashSet<&str>,
    ) -> impl Iterator<Item = RemoteFile> {
        self.packages
            .iter()
            .filter(|p| pkg_to_download.contains(p.get_name()))
            .flat_map(|pkg| pkg.remote_files())
    }

    // dato un pacchetto fa l'append delle dipendenze
    // sul vettore fornito
    // per sostituire i nomi .ARCH dei pacchetti
    pub fn collect_dependencies(&self, pkg_name: &'a str, archive_out: &mut Vec<&'a str>) {
        let Some(pkg) = self.get_package(pkg_name) else {
            eprintln!("Messaggio dalla funzione 'collect_dependencies()':");
            eprintln!("strano, il pacchetto richiesto {pkg_name} non esiste!");
            return;
        };

        let Some(deps) = pkg.dependencies_as_slice() else {
            return;
        };

        archive_out.extend(deps.iter().filter_map(|&dep_name| {
            if let Some(base_name) = dep_name.strip_suffix(".ARCH") {
                let destination_pkg_name = format!("{base_name}.{}", self.target_arch);
                self.get_package(&destination_pkg_name)
                    .map(|p| p.get_name())
            } else {
                Some(dep_name)
            }
        }));
    }

    pub fn len(&self) -> usize {
        self.packages.len()
    }

    pub fn get_package(&self, pkg_name: &str) -> Option<&Package<'a>> {
        self.index.get(pkg_name).map(|&idx| &self.packages[idx])
    }

    // risoluzione delle dipendenze
    pub fn resolve_dep(&self, entry_packages: &[&'a str]) -> AHashSet<&str> {
        let mut worklist = Vec::with_capacity(256);
        worklist.extend(entry_packages.iter());

        let mut visited = AHashSet::with_capacity(5500);

        // ciclo principale
        while let Some(last) = worklist.pop() {
            if visited.insert(last) {
                // il pacchetto non è ancora stato inserito
                // al suo posto metti le sue dipendenze
                if let Some(pkg) = self.get_package(last) {
                    self.collect_dependencies(pkg.get_name(), &mut worklist);
                };
            };
        }
        visited
    }

    /// parse the database texlive.tlpdb
    pub fn from_tlpdb(
        tlpdb_content: &'a str,
        target_arch: &'static str,
        include_doc: bool,
        include_src: bool,
        mirror_url: &'a str,
        skip_arch_filter: bool,
    ) -> Self {
        let t_parse = Instant::now();

        // eliminate the ending newlines
        let tlpdb_content = tlpdb_content.trim_end();

        let mut vec_packages = Vec::with_capacity(Parser::PKG_COUNT);
        let mut map_index = AHashMap::with_capacity(Parser::PKG_COUNT);

        // parsing of non-blank consecutive lines of text
        let iter_block = tlpdb_content.split("\n\n");
        let mut iter_block = iter_block.peekable();

        // data expected from parsing
        let mut options = ConfigData::default();

        // lettura dati di configurazione
        while let Some(lines_block) = iter_block.peek() {
            if lines_block.starts_with("name 00texlive.") {
                let lines_block = iter_block.next().unwrap(); // ora sì, consuma
                parse_00texlive_block(lines_block, &mut options);
            } else {
                break; // non consumato: resta disponibile per il ciclo successivo
            }
        }

        // lettura del resto dei pacchetti
        for lines_block in iter_block {
            if let Some(package) = Package::parse_from_str(
                lines_block,
                target_arch,
                skip_arch_filter,
                include_doc,
                include_src,
            ) {
                let name = package.get_name();

                map_index.insert(name, vec_packages.len());
                vec_packages.push(package);
            }
        }
        println!("Total time for parsing {:?}", t_parse.elapsed());

        Database {
            packages: vec_packages,
            index: map_index,
            target_arch,
            options,
            mirror_url,
            include_doc,
            include_src,
        }
    }
}

#[derive(Default)]
pub struct ConfigData<'a> {
    // config
    pub release: u16,
    min_release: u16,
    frozen: bool,
    // installation
    opt_autobackup: u32,
    opt_backupdir: &'a str,
    opt_create_formats: bool,
    opt_desktop_integration: bool,
    opt_file_assocs: u8,
    opt_generate_updmap: bool,
    opt_install_docfiles: bool,
    opt_install_srcfiles: bool,
    opt_location: &'a str,
    opt_post_code: bool,
    opt_sys_bin: &'a str,
    opt_sys_info: &'a str,
    opt_sys_man: &'a str,
    opt_w32_multi_user: bool,
    setting_available_architectures: Vec<&'a str>,
}

fn parse_00texlive_block<'a>(lines_block: &'a str, config_data: &mut ConfigData<'a>) {
    let Some((first_line, rest_lines)) = lines_block.split_once("\n") else {
        unreachable!("blocco malformato: deve contenere più righe: {lines_block}");
    };
    match first_line {
        "name 00texlive.config" => parse_00config(rest_lines, config_data),
        "name 00texlive.installation" => parse_00installation(rest_lines, config_data),
        "name 00texlive.image" => {}
        "name 00texlive.installer" => {}
        _ => unreachable!("blocco 00texlive non previsto: {first_line}"),
    }
}

fn parse_01_bool(val: &str) -> bool {
    match val {
        "1" => true,
        "0" => false,
        _ => unreachable!("valore booleano inatteso per: '{val}'"),
    }
}

/*
depend container_format/xz
depend container_split_doc_files/1
depend container_split_src_files/1
depend frozen/0
depend minrelease/2016
depend release/2026
depend revision/79748
*/
fn parse_00config(lines: &str, config_data: &mut ConfigData) {
    for line in lines.lines() {
        // estrai la parte dopo "depend ", altrimenti passa oltre
        let Some(option_item) = line.strip_prefix("depend ") else {
            continue;
        };

        // separa chiave/valore allo slash, altrimenti blocca
        let Some((option, value)) = option_item.split_once('/') else {
            unreachable!("attenzione: formato chiave/valore malformato: {option_item}")
        };

        // gestione dei valori
        match option {
            "minrelease" => {
                config_data.min_release = value.trim().parse().unwrap_or_else(|_| {
                    unreachable!("attenzione: valore minrelease non numerico ('{value}')");
                });
            }
            "release" => {
                config_data.release = value.trim().parse().unwrap_or_else(|_| {
                    unreachable!("attenzione: valore release non numerico ('{value}')");
                });
            }
            "frozen" => config_data.frozen = parse_01_bool(value),
            _ => {}
        }
    }
}

/*
depend opt_autobackup:1
depend opt_backupdir:tlpkg/backups
depend opt_create_formats:1
depend opt_desktop_integration:1
depend opt_file_assocs:1
depend opt_generate_updmap:0
depend opt_install_docfiles:1
depend opt_install_srcfiles:1
depend opt_location:__MASTER__
depend opt_post_code:1
depend opt_sys_bin:/usr/local/bin
depend opt_sys_info:/usr/local/share/info
depend opt_sys_man:/usr/local/share/man
depend opt_w32_multi_user:1
depend setting_available_architectures:aarch64-linux amd64-freebsd amd64-netbsd armhf-linux i386-freebsd i386-linux i386-netbsd universal-darwin windows x86_64-cygwin x86_64-darwinlegacy x86_64-linux x86_64-linuxmusl
 */
fn parse_00installation<'a>(lines: &'a str, options: &mut ConfigData<'a>) {
    for line in lines.lines() {
        // estrai la parte dopo "depend ", altrimenti passa alla riga successiva
        let Some(item) = line.trim().strip_prefix("depend ") else {
            continue;
        };

        let Some((key, value)) = item.split_once(':') else {
            unreachable!("attenzione: installation option malformata: {item}")
        };

        match key {
            "opt_autobackup" => {
                options.opt_autobackup = value.parse().unwrap_or_else(|_| {
                    unreachable!("valore numerico inatteso per opt_autobackup: '{value}'");
                });
            }
            "opt_backupdir" => options.opt_backupdir = value,
            "opt_create_formats" => options.opt_create_formats = parse_01_bool(value),
            "opt_desktop_integration" => options.opt_desktop_integration = parse_01_bool(value),
            "opt_file_assocs" => {
                options.opt_file_assocs = value.parse().unwrap_or_else(|_| {
                    unreachable!("valore numerico inatteso per opt_file_assocs: '{value}'");
                })
            }
            "opt_generate_updmap" => options.opt_generate_updmap = parse_01_bool(value),
            "opt_install_docfiles" => options.opt_install_docfiles = parse_01_bool(value),
            "opt_install_srcfiles" => options.opt_install_srcfiles = parse_01_bool(value),
            "opt_location" => options.opt_location = value,
            "opt_post_code" => options.opt_post_code = parse_01_bool(value),
            "opt_sys_bin" => options.opt_sys_bin = value,
            "opt_sys_info" => options.opt_sys_info = value,
            "opt_sys_man" => options.opt_sys_man = value,
            "opt_w32_multi_user" => options.opt_w32_multi_user = parse_01_bool(value),
            "setting_available_architectures" => {
                options.setting_available_architectures = value.split_whitespace().collect();
            }
            _ => {}
        }
    }
}
