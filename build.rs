// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at http://mozilla.org/MPL/2.0/.

// SPDX-License-Identifier: MPL-2.0

use std::env;

fn main() {
    // cargo imposta sempre la variabile TARGET durante la build
    let target = env::var("TARGET").expect("Variabile d'ambiente TARGET non trovata");

    let tl_arch = if target.contains("windows") {
        "windows"
    } else if target.contains("apple-darwin") {
        "universal-darwin"
    } else if target.contains("linux-gnu") {
        if target.starts_with("x86_64") {
            "x86_64-linux"
        } else if target.starts_with("aarch64") {
            "aarch64-linux"
        } else if target.starts_with("i686") {
            "i386-linux"
        } else {
            "unknown"
        }
    } else if target.contains("linux-musl") {
        if target.starts_with("x86_64") {
            "x86_64-linuxmusl"
        } else if target.starts_with("aarch64") {
            "aarch64-linuxmusl"
        } else {
            "unknown"
        }
    } else if target.contains("freebsd") {
        if target.starts_with("x86_64") {
            "amd64-freebsd"
        } else if target.starts_with("i686") {
            "i386-freebsd"
        } else {
            "unknown"
        }
    } else {
        "unknown"
    };

    if tl_arch == "unknown" {
        // avvisa il compilatore nel caso in cui qualcuno provi a compilare per un target non supportato
        println!("cargo:warning=Target '{}' has not a corresponding TeX Live architecture.", target);
    }

    // esporta il risultato come variabile d'ambiente leggibile dal codice sorgente
    println!("cargo:rustc-env=TLX_ARCH={}", tl_arch);
    
    // riavvia lo script di build solo se questo file viene modificato
    println!("cargo:rerun-if-changed=build.rs");
}
