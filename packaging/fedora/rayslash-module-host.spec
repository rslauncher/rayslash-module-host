Name:           rayslash-module-host
Version:        0.1.2
Release:        1%{?dist}
Summary:        Optional sandbox host for RaySlash WASM modules
License:        MIT
URL:            https://github.com/rslauncher/rayslash-module-host
Source1:        %{url}/raw/v%{version}/LICENSE

%ifarch x86_64
%global archive_target x86_64-unknown-linux-gnu
%endif
%ifarch aarch64
%global archive_target aarch64-unknown-linux-gnu
%endif
Source0:        %{url}/releases/download/v%{version}/rayslash-module-host-v%{version}-%{archive_target}.tar.xz
ExclusiveArch:  x86_64 aarch64

%description
Separately installable, no-WASI Wasmtime process used only when a RaySlash WASM
module is installed. The core launcher does not require this package.

%prep
%setup -q -n rayslash-module-host-v%{version}-%{archive_target}

%install
install -Dm0755 rayslash-module-host %{buildroot}%{_libexecdir}/rayslash/rayslash-module-host
install -Dm0644 %{SOURCE1} %{buildroot}%{_licensedir}/%{name}/LICENSE

%files
%license %{_licensedir}/%{name}/LICENSE
%{_libexecdir}/rayslash/rayslash-module-host

%changelog
* Sun Jul 13 2026 RaySlash contributors - 0.1.2-1
- Enforce exact network origins across redirects.

* Sun Jul 12 2026 RaySlash contributors - 0.1.1-1
- Validate all module result fields and typed actions at the host boundary.

* Sun Jul 12 2026 RaySlash contributors - 0.1.0-1
- Initial optional host package
