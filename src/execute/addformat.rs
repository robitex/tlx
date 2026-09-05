// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at http://mozilla.org/MPL/2.0/.

// SPDX-License-Identifier: MPL-2.0

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AddFormat<'a> {
    pub pkg_name: &'a str,
    pub name: &'a str,
    pub engine: &'a str,
    pub mode: Option<&'a str>,
    pub patterns: Option<&'a str>,
    pub options: Option<&'a str>,
    pub fmttriggers: Vec<&'a str>,
}

impl<'a> AddFormat<'a> {
    pub fn parse(pkg_name: &'a str, input: &'a str) -> Option<Self> {
        let mut name = None;
        let mut engine = None;
        let mut mode = None;
        let mut patterns = None;
        let mut options = None;
        let mut fmttriggers = Vec::new();

        for token in input.split_whitespace() {
            if let Some((key, value)) = token.split_once('=') {
                match key {
                    "name" if !value.is_empty() => name = Some(value),
                    "engine" if !value.is_empty() => engine = Some(value),
                    "mode" if !value.is_empty() => mode = Some(value),
                    "patterns" if !value.is_empty() => patterns = Some(value),
                    "options" if !value.is_empty() => options = Some(value),
                    "fmttriggers" if !value.is_empty() => {
                        fmttriggers = value.split(',').collect();
                    }
                    _ => {}
                }
            }
        }

        Some(AddFormat {
            pkg_name,
            name: name?,
            engine: engine?,
            mode,
            patterns,
            options,
            fmttriggers,
        })
    }
}
