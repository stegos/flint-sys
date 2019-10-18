use std::env;
use std::fs;
//use std::io::{BufRead, BufReader, BufWriter, Result as IoResult, Write};
use std::path::{Path, PathBuf};
use std::process::Command;
const FLINT_DIR: &'static str = "flint2";

macro_rules! makefile_template {
    ($($arg:tt)*) => (format!(
r#"
SHELL=/bin/sh

FLINT_STATIC=1
FLINT_SHARED=0
FLINT_SOLIB=0
EXEEXT=
PREFIX={prefix}

WANT_NTL=0

INCS=-I$(CURDIR) -I{gmp_mpfr_include_dir}
LIBS=-L$(CURDIR) -L{gmp_mpfr_lib_dir} -lflint -lmpfr -lgmp -lm -lpthread
LIBS2=-L$(CURDIR) -L{gmp_mpfr_lib_dir} -lmpfr -lgmp -lm -lpthread

CC={cc}
CXX={cxx}
AR=ar
LDCONFIG=ldconfig

CFLAGS=-ansi -pedantic -Wall {cflags}
CXXFLAGS=-ansi -pedantic -Wall {cxxflags}
ABI_FLAG=
PIC_FLAG=-fPIC
EXTRA_SHARED_FLAGS=-Wl,-soname,libflint.so.13

DLPATH=LD_LIBRARY_PATH
DLPATH_ADD=$(CURDIR)
EXTENSIONS=
EXTRA_BUILD_DIRS=flintxx
"#,
$($arg)*
))
}

macro_rules! config_template {
    () => {
        r#"
#define POPCNT_INTRINSICS
#define HAVE_BLAS 0
#define HAVE_TLS 1
#define HAVE_FENV 1
#define HAVE_PTHREAD 1
#define HAVE_GC 0
#define FLINT_REENTRANT 0
#define WANT_ASSERT 0
#define FLINT_DLL
"#
    };
}

fn main() {
    // Get CC, CFLAGS and HOST from `cc` crate.
    let compiler = cc::Build::new().get_compiler();
    let cc = match &compiler.cc_env() {
        cc if cc.is_empty() => compiler
            .path()
            .to_str()
            .expect("Unprintable CC")
            .to_string(),
        cc => cc.to_str().expect("Unprintable CC").to_string(),
    };
    let cflags = compiler
        .cflags_env()
        .to_str()
        .expect("Unprintable CFLAGS")
        .to_string();
    let compiler = cc::Build::new().cpp(true).get_compiler();
    let cxx = match &compiler.cc_env() {
        cxx if cc.is_empty() => compiler
            .path()
            .to_str()
            .expect("Unprintable CXX")
            .to_string(),
        cxx => cxx.to_str().expect("Unprintable CXX").to_string(),
    };
    let cxxflags = compiler
        .cflags_env()
        .to_str()
        .expect("Unprintable CXXFLAGS")
        .to_string();

    let num_jobs = var("NUM_JOBS");
    let bits = var("CARGO_CFG_TARGET_POINTER_WIDTH");
    let gmp_mpfr_include_dir = var("DEP_GMP_INCLUDE_DIR");
    let gmp_mpfr_lib_dir = var("DEP_GMP_LIB_DIR");

    //
    // Create directories.
    //
    let src_dir = PathBuf::from(var("CARGO_MANIFEST_DIR"));
    if src_dir.to_str().is_none() {
        panic!("{:?} contains unsupported symbols", src_dir);
    }
    let out_dir = PathBuf::from(var("OUT_DIR"));
    if out_dir.to_str().is_none() {
        panic!("{:?} contains unsupported symbols", out_dir);
    }
    let build_dir = out_dir.join("build");
    let lib_dir = out_dir.join("lib");
    let include_dir = out_dir.join("include");
    create_dir(&build_dir);

    //
    // Copy sources.
    //
    let flint_dir = build_dir.join(FLINT_DIR);
    copy_dir(&src_dir.join(&FLINT_DIR), &flint_dir);

    //
    // Configure.
    //

    // Write Makefile.
    let makefile = flint_dir.join("Makefile");
    let makefile_in = flint_dir.join("Makefile.in");
    let mut makefile_content = makefile_template!(
        prefix = out_dir.to_str().unwrap(),
        cc = cc,
        cxx = cxx,
        cflags = cflags,
        cxxflags = cxxflags,
        gmp_mpfr_include_dir = gmp_mpfr_include_dir,
        gmp_mpfr_lib_dir = gmp_mpfr_lib_dir
    );
    makefile_content.push_str(&read_from_file(&makefile_in));
    write_to_file(&makefile, &makefile_content);

    // Write config.h.
    let config = flint_dir.join("config.h");
    let config_content = config_template!();
    write_to_file(&config, &config_content);

    // Copy fft_tuning.h
    copy_file(
        &flint_dir.join(format!("fft_tuning{}.in", bits)),
        &flint_dir.join("fft_tuning.h"),
    );

    // Copy fmpz.c
    copy_file(
        &flint_dir.join("fmpz").join("link").join("fmpz_single.c"),
        &flint_dir.join("fmpz").join("fmpz.c"),
    );

    // Copy fmpz-conversions.h.
    copy_file(
        &flint_dir.join("fmpz-conversions-reentrant.in"),
        &flint_dir.join("fmpz-conversions.h"),
    );

    //
    // Run `make`.
    //
    let mut make_cmd = Command::new("make");
    make_cmd
        .current_dir(&flint_dir)
        .arg("-j")
        .arg(num_jobs)
        .arg("install");
    execute(make_cmd);

    let flint_a = lib_dir.join("libflint.a");
    let flint_h = include_dir.join("flint").join("flint.h");
    if !flint_a.exists() {
        panic!("Missing {:?}", flint_a);
    }
    if !flint_h.exists() {
        panic!("Missing {:?}", flint_h);
    }

    println!("cargo:out_dir={}", out_dir.to_str().unwrap());
    println!("cargo:lib_dir={}", lib_dir.to_str().unwrap());
    println!("cargo:include_dir={}", include_dir.to_str().unwrap());
    println!(
        "cargo:rustc-link-search=native={}",
        lib_dir.to_str().unwrap()
    );
    println!("cargo:rustc-link-lib=static=flint");
}

fn var(name: &str) -> String {
    env::var(name)
        .map_err(|_e| format!("Missing {}", name))
        .unwrap()
}

fn var_or_default(name: &str) -> String {
    env::var(name).unwrap_or_default()
}

fn create_dir(dst: &Path) {
    println!("Create directory {:?}", dst);
    fs::create_dir_all(dst) //
        .map_err(|e| format!("Failed to create {:?}: {}", dst, e))
        .unwrap();
}

fn read_from_file(src: &Path) -> String {
    fs::read_to_string(src)
        .map_err(|e| format!("Failed to read {:?}: {}", src, e))
        .unwrap()
}

fn write_to_file(dst: &Path, content: &str) {
    if dst.exists() {
        let old_content = read_from_file(&dst);
        if &old_content == content {
            println!("File {:?} is up to date, skipping", dst);
            return;
        }
    }
    println!("Write file {:?}", dst);
    fs::write(dst, content)
        .map_err(|e| format!("Failed to write {:?}: {}", dst, e))
        .unwrap();
}

fn copy_file(src: &Path, dst: &Path) {
    let content = read_from_file(&src);
    write_to_file(dst, &content);
}

fn copy_dir(src: &Path, dst: &Path) {
    if dst.exists() {
        println!("Directory {:?} is up to date, skipping", dst);
        return;
    }
    println!("Copy directory {:?} to {:?}", src, dst);
    let mut options = fs_extra::dir::CopyOptions::new();
    options.copy_inside = true;
    fs_extra::dir::copy(src, dst, &options) //
        .map_err(|e| format!("Failed to copy directory {:?} to {:?}: {}", src, dst, e))
        .unwrap();
}

fn execute(mut command: Command) {
    println!("Execute {:?}", command);
    let status = command
        .status()
        .map_err(|e| format!("Failed to execute {:?}: {}", command, e))
        .unwrap();
    if !status.success() {
        if let Some(code) = status.code() {
            panic!("Command {:?} failed: code={}", command, code);
        } else {
            panic!("Command {:?}", command);
        }
    }
}
