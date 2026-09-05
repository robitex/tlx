// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at http://mozilla.org/MPL/2.0/.

// SPDX-License-Identifier: MPL-2.0

use std::io::Write;

///! modulo dedicato all'esecuzione della direttiva
///! 'execute AddHyphen ...' di post-installazione

/// rappresenta una direttiva 'execute AddHyphen'
pub struct AddHyphen<'a> {
    pkg_name: &'a str,
    name: &'a str,
    file: &'a str,
    lefthyphenmin: Option<u8>,
    righthyphenmin: Option<u8>,
    synonyms: Vec<&'a str>,
    file_patterns: Option<&'a str>,
    file_exceptions: Option<&'a str>,
    luaspecial: Option<&'a str>,
}

impl<'a> AddHyphen<'a> {
    pub fn parse(pkg_name: &'a str, input: &'a str) -> Option<Self> {
        let mut name = None;
        let mut file = None;
        let mut lefthyphenmin = None;
        let mut righthyphenmin = None;
        let mut synonyms = Vec::new();
        let mut file_patterns = None;
        let mut file_exceptions = None;
        let mut luaspecial = None;

        for token in input.split_whitespace() {
            if let Some((key, value)) = token.split_once('=') {
                match key {
                    "name" => name = Some(value),
                    "file" => file = Some(value),
                    "lefthyphenmin" => lefthyphenmin = value.parse().ok(),
                    "righthyphenmin" => righthyphenmin = value.parse().ok(),
                    "synonyms" if !value.is_empty() => synonyms = value.split(',').collect(),
                    "file_patterns" if !value.is_empty() => file_patterns = Some(value),
                    "file_exceptions" if !value.is_empty() => file_exceptions = Some(value),
                    "luaspecial" if !value.is_empty() => luaspecial = Some(value),
                    _ => {}
                }
            }
        }

        Some(AddHyphen {
            pkg_name,
            name: name?,
            file: file?,
            lefthyphenmin,
            righthyphenmin,
            synonyms,
            file_patterns,
            file_exceptions,
            luaspecial,
        })
    }

    pub fn write_language_dat(&self, out: &mut impl Write) -> std::io::Result<()> {
        writeln!(out, "% from {}:", self.pkg_name)?;
        writeln!(out, "{} {}", self.name, self.file)?;

        for &syn in &self.synonyms {
            writeln!(out, "={syn}")?;
        }

        Ok(())
    }

    pub fn write_language_def(&self, out: &mut impl Write) -> std::io::Result<()> {
        let left = self.lefthyphenmin.unwrap_or(0);
        let right = self.righthyphenmin.unwrap_or(0);

        writeln!(out, "% from {}:", self.pkg_name)?;
        write!(out, "\\addlanguage")?;
        write!(out, "{{{}}}", self.name)?; // arg 1
        write!(out, "{{{}}}", self.file)?; // arg 2
        write!(out, "{{}}")?; // arg 3
        write!(out, "{{{left}}}")?; // arg 4
        writeln!(out, "{{{right}}}")?; // arg 5

        for &syn in &self.synonyms {
            write!(out, "\\addlanguage")?;
            write!(out, "{{{syn}}}")?; // arg 1
            write!(out, "{{{}}}", self.file)?; // arg 2
            write!(out, "{{}}")?; // arg 3
            write!(out, "{{{left}}}")?; // arg 4
            writeln!(out, "{{{right}}}")?; // arg 5
        }

        Ok(())
    }

    pub fn write_language_dat_lua(&self, out: &mut impl Write) -> std::io::Result<()> {
        let left = self.lefthyphenmin.unwrap_or(0);
        let right = self.righthyphenmin.unwrap_or(0);

        writeln!(out, "-- from {}:", self.pkg_name)?;
        writeln!(out, "\t['{}'] = {{", self.name)?;
        writeln!(out, "\t\tloader = '{}',", self.file)?;
        writeln!(out, "\t\tlefthyphenmin = {left},")?;
        writeln!(out, "\t\trighthyphenmin = {right},")?;

        write!(out, "\t\tsynonyms = {{ ")?;
        for (i, syn) in self.synonyms.iter().enumerate() {
            if i > 0 {
                write!(out, ", ")?;
            }
            write!(out, "'{syn}'")?;
        }
        writeln!(out, " }},")?;

        if let Some(p) = self.file_patterns {
            writeln!(out, "\t\tpatterns = '{p}',")?;
        }
        if self.file_patterns.is_some() || self.file_exceptions.is_some() {
            let excep = self.file_exceptions.unwrap_or("");
            writeln!(out, "\t\thyphenation = '{excep}',")?;
        }

        if let Some(s) = self.luaspecial {
            writeln!(out, "\t\tspecial = '{s}',")?;
        }

        writeln!(out, "\t}},")?;
        Ok(())
    }
}
