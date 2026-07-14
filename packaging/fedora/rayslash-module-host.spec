Name:           rayslash-module-host
Version:        0.1.2
Release:        1%{?dist}
Summary:        Sandbox host for RaySlash WASM modules
License:        MIT
URL:            https://github.com/rslauncher/rayslash-module-host
Source1:        LICENSE

# Release archives contain the final verified executable. Preserve it exactly
# and do not emit empty debug packages for this prebuilt-binary package.
%global debug_package %{nil}
%global __strip /bin/true

%ifarch x86_64
%global archive_target x86_64-unknown-linux-gnu
%endif
%ifarch aarch64
%global archive_target aarch64-unknown-linux-gnu
%endif
# packaging/fedora/build-release-rpm.sh downloads these immutable release-tag
# inputs and verifies both the pinned checksums and the published sidecar before
# invoking rpmbuild. Keep rpmbuild itself network-free.
Source0:        rayslash-module-host-v%{version}-%{archive_target}.tar.xz
ExclusiveArch:  x86_64 aarch64

%description
No-WASI Wasmtime process used to install and run RaySlash WASM modules. RaySlash
application packages depend on this separately maintained runtime while module
packages themselves remain uninstalled until selected by the user.

%prep
%setup -q -n rayslash-module-host-v%{version}-%{archive_target}

%install
install -Dm0755 rayslash-module-host %{buildroot}%{_libexecdir}/rayslash/rayslash-module-host
install -Dm0644 %{SOURCE1} %{buildroot}%{_licensedir}/%{name}/LICENSE

%files
%license %{_licensedir}/%{name}/LICENSE
%{_libexecdir}/rayslash/rayslash-module-host

%changelog
* Mon Jul 13 2026 RaySlash contributors - 0.1.2-1
- Enforce exact network origins across redirects.

* Sun Jul 12 2026 RaySlash contributors - 0.1.1-1
- Validate all module result fields and typed actions at the host boundary.

* Sun Jul 12 2026 RaySlash contributors - 0.1.0-1
- Initial optional host package
