Name:           83sc-control
Version:        1.0.0
Release:        1%{?dist}
Summary:        Thermal, power and fan control for the Lenovo LOQ Essential 15IRX11 (83SC)

License:        GPL-2.0-or-later
URL:            https://github.com/blussyya/83sc-control
Source0:        %{name}-%{version}.tar.gz

BuildRequires:  cargo
BuildRequires:  gcc
Requires:       python3
Requires:       systemd
Recommends:     intel-undervolt

%description
Power limits, fan curve, undervolt and keyboard controls for the Lenovo LOQ
Essential 15IRX11 (DMI 83SC, i7-13650HX / RTX 5050), with a GUI and CLI tools.
Settings are replayed at boot by a systemd service. Fan control requires
legion_laptop carrying the 83SC fixes (LenovoLegionLinux >= v0.0.26).

%prep
%autosetup

%build
cargo build --release --manifest-path gui/Cargo.toml
cargo build --release --manifest-path kbd-idle/Cargo.toml

%install
install -Dm755 gui/target/release/legion83-gui        %{buildroot}%{_bindir}/legion83-gui
install -Dm755 kbd-idle/target/release/83sc-kbd-idle  %{buildroot}%{_bindir}/83sc-kbd-idle
for t in 83sc 83sc-diag 83sc-fan 83sc-snap; do
    install -Dm755 bin/$t %{buildroot}%{_bindir}/$t
done
install -Dm755 helper/helper.py             %{buildroot}%{_prefix}/lib/83sc-control/helper.py
install -Dm755 systemd/83sc-thermal.sh      %{buildroot}%{_prefix}/lib/83sc-control/83sc-thermal.sh
install -Dm755 systemd/83sc-driver-guard.sh %{buildroot}%{_prefix}/lib/83sc-control/83sc-driver-guard.sh
for u in 83sc-thermal 83sc-driver-guard; do
    sed 's#/usr/local/lib/#/usr/lib/#' systemd/$u.service > $u.service.out
    install -Dm644 $u.service.out %{buildroot}%{_unitdir}/$u.service
done
sed 's#%h/.local/bin/#/usr/bin/#' systemd/83sc-kbd-idle.service > kbd.service.out
install -Dm644 kbd.service.out %{buildroot}%{_userunitdir}/83sc-kbd-idle.service
install -Dm644 gui/83sc-control.desktop %{buildroot}%{_datadir}/applications/83sc-control.desktop
sed 's/%%ADMINGROUP%%/%%wheel/' packaging/83sc-control.sudoers > sudoers.out
install -Dm440 sudoers.out %{buildroot}%{_sysconfdir}/sudoers.d/83sc-control

%files
%license LICENSE
%doc README.md docs/PORTABILITY.md
%{_bindir}/83sc
%{_bindir}/83sc-diag
%{_bindir}/83sc-fan
%{_bindir}/83sc-snap
%{_bindir}/83sc-kbd-idle
%{_bindir}/legion83-gui
%{_prefix}/lib/83sc-control/
%{_unitdir}/83sc-thermal.service
%{_unitdir}/83sc-driver-guard.service
%{_userunitdir}/83sc-kbd-idle.service
%{_datadir}/applications/83sc-control.desktop
%config(noreplace) %attr(0440,root,root) %{_sysconfdir}/sudoers.d/83sc-control

%post
%systemd_post 83sc-thermal.service 83sc-driver-guard.service

%preun
%systemd_preun 83sc-thermal.service 83sc-driver-guard.service

%postun
%systemd_postun_with_restart 83sc-thermal.service

%changelog
* Sat Sep 12 2026 blussyya <https://github.com/blussyya> - 1.0.0-1
- Initial package.
