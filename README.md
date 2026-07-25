# rayslash module host

Required sandbox infrastructure for installable rayslash modules. Official rayslash RPM, DEB, AppImage, and Flatpak packages embed the matching pinned host release, so users can browse and install modules without a separate setup step. The host ships no official or community module code by itself.

The host loads WebAssembly components implementing rayslash module API v1. API v1 does not install declarative packages. It intentionally provides no WASI filesystem, socket, environment, or process interfaces. Modules receive only bounded rayslash host imports, while the host itself runs as a persistent, launcher-managed child process over newline-delimited JSON IPC.

Security limits include fuel, linear memory, result/query sizes, exact HTTPS origin allowlists, HTTP time/body/header caps, cache-key/path validation, and atomic cache writes. The parent launcher remains responsible for process deadlines and typed action approval.

The host persists Wasmtime's compiled-code cache below the module cache directory, so a component pays native compilation once for a compatible host/Wasmtime/CPU configuration rather than on every fresh process. One reusable HTTP agent also pools connections across requests within a host process. These caches do not widen module capabilities: components still receive only the API imports and allowlisted origins supplied by the launcher.

Release archives and compatibility Fedora RPMs are published for `x86_64` and `aarch64` on the matching [GitHub Release](https://github.com/rslauncher/rayslash-module-host/releases). The rayslash package workflow verifies the immutable archive digest before embedding the executable at `/usr/libexec/rayslash/rayslash-module-host`. Standalone Fedora and Arch recipes remain useful for third-party packaging and older rayslash releases. Developers may instead set `RAYSLASH_MODULE_HOST` to an absolute host binary path.
