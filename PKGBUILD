# Maintainer: nxoji
pkgname=waytime
pkgver=0.2.0
pkgrel=1
pkgdesc="Zero-overhead screen time tracker for Wayland & KDE Plasma"
arch=('x86_64')
url="https://github.com/nxoji/waytime"
license=('MIT')
depends=('gcc-libs' 'glibc')
makedepends=('cargo')
source=("$pkgname-$pkgver.tar.gz::$url/archive/refs/tags/v$pkgver.tar.gz")
sha256sums=('SKIP')

prepare() {
  cd "$pkgname-$pkgver"
  cargo fetch --locked --target "$(rustc -vV | sed -n 's/host: //p')"
}

build() {
  cd "$pkgname-$pkgver"
  export RUSTUP_TOOLCHAIN=stable
  export CARGO_TARGET_DIR=target
  unset CFLAGS CXXFLAGS LDFLAGS RUSTFLAGS
  cargo build --release --frozen
}

check() {
  cd "$pkgname-$pkgver"
  unset CFLAGS CXXFLAGS LDFLAGS RUSTFLAGS
  cargo test --frozen
}

package() {
  cd "$pkgname-$pkgver"
  install -Dm755 "target/release/waytime" "$pkgdir/usr/bin/waytime"
  install -Dm644 "waytime.service" "$pkgdir/usr/lib/systemd/user/waytime.service"
  install -Dm644 "LICENSE" "$pkgdir/usr/share/licenses/$pkgname/LICENSE"
}
