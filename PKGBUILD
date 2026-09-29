# Maintainer: nxoji
pkgname=waytime
pkgver=0.1.0
pkgrel=1
pkgdesc="Zero-overhead Wayland/KDE screen time tracker in Rust"
arch=('x86_64')
url="https://github.com/nxoji/waytime"
license=('MIT')
depends=('gcc-libs' 'glibc')
makedepends=('cargo')

build() {
  cd "$startdir"
  export RUSTUP_TOOLCHAIN=stable
  unset CFLAGS CXXFLAGS LDFLAGS RUSTFLAGS
  cargo build --release --locked
}

package() {
  cd "$startdir"
  install -Dm755 "target/release/waytime" "$pkgdir/usr/bin/waytime"
  install -Dm644 "waytime.service" "$pkgdir/usr/lib/systemd/user/waytime.service"
  install -Dm644 "LICENSE" "$pkgdir/usr/share/licenses/$pkgname/LICENSE"
}
