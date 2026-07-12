# rayslash module host

Optional sandbox host for executable rayslash modules. Fresh rayslash installations and declarative modules do not require this binary.

The host loads WebAssembly components implementing rayslash module API v1. It intentionally provides no WASI filesystem, socket, environment, or process interfaces. Modules receive only bounded rayslash host imports, while the host itself runs as a disposable child process over newline-delimited JSON IPC.

Security limits include fuel, linear memory, result/query sizes, exact HTTPS origin allowlists, HTTP time/body/header caps, cache-key/path validation, and atomic cache writes. The parent launcher remains responsible for process deadlines and typed action approval.

Release archives are published for `x86_64` and `aarch64`. Fedora and Arch package recipes under `packaging/` install the binary at `/usr/libexec/rayslash/rayslash-module-host`, where the launcher discovers it automatically. Developers may instead set `RAYSLASH_MODULE_HOST` to an absolute host binary path.
