use std::{
    env, fs,
    path::{Path, PathBuf},
    process::Command,
};

pub fn source_dir() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("mbedtls")
}

pub fn version() -> &'static str {
    env!("CARGO_PKG_VERSION")
}

pub struct Build {
    out_dir: Option<PathBuf>,
    target: Option<String>,
    host: Option<String>,
}

pub struct Artifacts {
    include_dir: PathBuf,
    lib_dir: PathBuf,
    libs: Vec<String>,
}

impl Build {
    pub fn new() -> Build {
        Build {
            out_dir: env::var_os("OUT_DIR").map(|s| PathBuf::from(s).join("mbedtls-build")),
            target: env::var("TARGET").ok(),
            host: env::var("HOST").ok(),
        }
    }

    pub fn out_dir<P: AsRef<Path>>(&mut self, path: P) -> &mut Build {
        self.out_dir = Some(path.as_ref().to_path_buf());
        self
    }

    pub fn target(&mut self, target: &str) -> &mut Build {
        self.target = Some(target.to_string());
        self
    }

    pub fn host(&mut self, host: &str) -> &mut Build {
        self.host = Some(host.to_string());
        self
    }

    fn cmd_make(&self) -> Command {
        match &self.host.as_ref().expect("HOST dir not set")[..] {
            "x86_64-unknown-dragonfly" => Command::new("gmake"),
            "x86_64-unknown-freebsd" => Command::new("gmake"),
            _ => Command::new("make"),
        }
    }

    pub fn build(&mut self) -> Artifacts {
        let target = &self.target.as_ref().expect("TARGET dir not set")[..];
        let host = &self.host.as_ref().expect("HOST dir not set")[..];
        let out_dir = self.out_dir.as_ref().expect("OUT_DIR not set");
        let build_dir = out_dir.join("build");
        let install_dir = out_dir.join("install");

        if build_dir.exists() {
            fs::remove_dir_all(&build_dir).unwrap();
        }
        if install_dir.exists() {
            fs::remove_dir_all(&install_dir).unwrap();
        }

        let inner_dir = build_dir.join("src");
        fs::create_dir_all(&inner_dir).unwrap();
        cp_r(&source_dir(), &inner_dir);

        // Generate configuration here...

        let mut ios_isysroot: std::option::Option<String> = None;
        let mut build = self.cmd_make();

        let mut cc = cc::Build::new();
        cc.target(target).host(host).warnings(false).opt_level(3);

        let compiler = cc.get_compiler();
        build.env("CC", compiler.path());

        let path = compiler.path().to_str().unwrap();

        // Infer ar/ranlib tools from cross compilers if the it looks like
        // we're doing something like `foo-gcc` route that to `foo-ranlib`
        // as well.
        if path.ends_with("-gcc") && !target.contains("unknown-linux-musl") {
            let path = &path[..path.len() - 4];
            build.env("RANLIB", format!("{}-ranlib", path));
            build.env("AR", format!("{}-ar", path));
        }

        // Make sure we pass extra flags like `-ffunction-sections` and
        // other things like ARM codegen flags.
        let mut skip_next = false;
        let mut is_isysroot = false;
        let mut cflags = Vec::new();
        for arg in compiler.args() {
            // For whatever reason `-static` on MUSL seems to cause
            // issues...
            if target.contains("musl") && arg == "-static" {
                continue;
            }

            // cargo-lipo specifies this but mbedtls complains
            if target.contains("apple-ios") {
                if arg == "-arch" {
                    skip_next = true;
                    continue;
                }

                if arg == "-isysroot" {
                    is_isysroot = true;
                    continue;
                }

                if is_isysroot {
                    is_isysroot = false;
                    ios_isysroot = Some(arg.to_str().unwrap().to_string());
                    continue;
                }
            }

            if skip_next {
                skip_next = false;
                continue;
            }

            cflags.push(arg.clone().into_string().unwrap());
        }
        build.env("CFLAGS", cflags.join(" "));

        if target == "x64_64-apple-ios" {
            if let Some(ref isysr) = ios_isysroot {
                build.env(
                    "CC",
                    &format!(
                        "xcrun -sdk iphonesimulator cc -isysroot {}",
                        sanitize_sh(&Path::new(isysr))
                    ),
                );
            }
        }

        build.current_dir(&inner_dir);

        if let Some(s) = env::var_os("CARGO_MAKEFLAGS") {
            build.env("MAKEFLAGS", s);
        }

        if let Some(ref isysr) = ios_isysroot {
            let components: Vec<&str> = isysr.split("/SDKs/").collect();
            build.env("CROSS_TOP", components[0]);
            build.env("CROSS_SDK", components[1]);
        }

        self.run_command(build, "building mbedtls");

        let mut install = self.cmd_make();
        install.arg("install");
        install.current_dir(&inner_dir);
        install.arg(format!("DESTDIR={}", install_dir.display()));
        self.run_command(install, "installing mbedtls");

        let libs = vec![
            "mbedtls".to_string(),
            "mbedx509".to_string(),
            "mbedcrypto".to_string(),
        ];
        fs::remove_dir_all(&inner_dir).unwrap();

        Artifacts {
            lib_dir: install_dir.join("lib"),
            include_dir: install_dir.join("include"),
            libs: libs,
        }
    }

    fn run_command(&self, mut command: Command, desc: &str) {
        println!("running {:?}", command);
        let status = command.status().unwrap();
        if !status.success() {
            panic!(
                "


Error {}:
    Command: {:?}
    Exit status: {}


    ",
                desc, command, status
            );
        }
    }
}

fn cp_r(src: &Path, dst: &Path) {
    for f in fs::read_dir(src).unwrap() {
        let f = f.unwrap();
        let path = f.path();
        let name = path.file_name().unwrap();

        // Skip git metadata as it's been known to cause issues (#26) and
        // otherwise shouldn't be required
        if name.to_str() == Some(".git") {
            continue;
        }

        let dst = dst.join(name);
        if f.file_type().unwrap().is_dir() {
            fs::create_dir_all(&dst).unwrap();
            cp_r(&path, &dst);
        } else {
            let _ = fs::remove_file(&dst);
            fs::copy(&path, &dst).unwrap();
        }
    }
}

fn sanitize_sh(path: &Path) -> String {
    return path.to_str().unwrap().to_string();
}

impl Artifacts {
    pub fn include_dir(&self) -> &Path {
        &self.include_dir
    }

    pub fn lib_dir(&self) -> &Path {
        &self.lib_dir
    }

    pub fn libs(&self) -> &[String] {
        &self.libs
    }

    pub fn print_cargo_metadata(&self) {
        println!("cargo:rustc-link-search=native={}", self.lib_dir.display());
        for lib in self.libs.iter() {
            println!("cargo:rustc-link-lib=static={}", lib);
        }
        println!("cargo:include={}", self.include_dir.display());
        println!("cargo:lib={}", self.lib_dir.display());
    }
}
