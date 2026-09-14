// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at http://mozilla.org/MPL/2.0/.

// SPDX-License-Identifier: MPL-2.0

const TL_ARCHITECTURES: [&str; 13] = [
    "windows",
    "x86_64-linux",
    "i386-freebsd",
    "i386-netbsd",
    "i386-linux",
    "universal-darwin",
    "amd64-freebsd",
    "amd64-netbsd",
    "armhf-linux",
    "x86_64-darwinlegacy",
    "x86_64-linuxmusl",
    "x86_64-cygwin",
    "aarch64-linux",
];

#[inline]
pub fn is_known_tl_arch(arch: &str) -> bool {
    TL_ARCHITECTURES.contains(&arch)
}

/// Recupera l'architettura TeX Live generata da build.rs a tempo di compilazione
pub fn get_target_arch() -> Option<&'static str> {
    match env!("TLX_ARCH") {
        "unknown" => None,
        arch => Some(arch),
    }
}
